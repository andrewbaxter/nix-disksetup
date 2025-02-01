use {
    crate::{
        config::{
            PinMode,
            PrivateImageKeyMode,
            SharedImageKeyMode,
        },
        util::{
            from_utf8,
            SimpleCommandExt,
        },
    },
    loga::{
        ea,
        DebugDisplay,
        Log,
        ResultContext,
    },
    openpgp_card_rpgp::CardSlot,
    pcsc::Context,
    pgp::Deserializable,
    rand::{
        prelude::SliceRandom,
        thread_rng,
    },
    std::{
        collections::{
            HashMap,
            HashSet,
        },
        fs::read,
        io::{
            stdin,
            Read,
        },
        path::Path,
        process::Command,
        thread::sleep,
        time::Duration,
    },
};

pub(crate) fn ask_password(message: &str) -> Result<String, loga::Error> {
    let raw =
        Command::new("systemd-ask-password")
            .arg("-n")
            .arg("--timeout=0")
            .arg(message)
            .simple()
            .run_stdout()
            .context("Error asking for password")?;
    return Ok(from_utf8(raw).context("Received password was invalid utf8")?.trim().to_string());
}

pub(crate) fn get_shared_image_key(key_mode: &SharedImageKeyMode, confirm: bool) -> Result<String, loga::Error> {
    match key_mode {
        SharedImageKeyMode::Stdin => {
            let mut data = Vec::new();
            stdin().read_to_end(&mut data).context("Failed to read shared image key from stdin")?;
            return Ok(from_utf8(data).context("Key must be utf-8")?);
        },
        SharedImageKeyMode::File(f) => {
            let data =
                read(f).context_with("Failed to read shared image key file", ea!(path = f.to_string_lossy()))?;
            return Ok(from_utf8(data).context("Key must be utf-8")?);
        },
        SharedImageKeyMode::Password => {
            let mut warning = None;
            loop {
                let mut prompt = String::new();
                if let Some(warning) = warning.take() {
                    prompt.push_str(warning);
                }
                prompt.push_str("Enter the password");
                let pw1 = ask_password(&prompt)?;
                if confirm {
                    let pw2 = ask_password("Confirm your password")?;
                    if pw1 != pw2 {
                        warning = Some("Passwords didn't match, please try again.\n");
                        continue;
                    }
                }
                return Ok(pw1);
            }
        },
    }
}

pub(crate) fn get_private_image_key(
    log: &Log,
    key_path: &Path,
    key_mode: &PrivateImageKeyMode,
) -> Result<String, loga::Error> {
    let encrypted =
        pgp::Message::from_armor_single(
            &mut read(key_path)
                .context_with("Error reading encrypted key", ea!(path = key_path.to_string_lossy()))?
                .as_slice(),
        )
            .context("Encrypted data isn't valid ASCII Armor")?
            .0;
    match key_mode {
        #[cfg(feature = "smartcard")]
        PrivateImageKeyMode::Smartcard { pin } => {
            let mut pcsc_context =
                pcsc::Context::establish(pcsc::Scope::User).context("Error setting up PCSC context")?;
            let mut watch: Vec<pcsc::ReaderState> = vec![];
            loop {
                let pin = match &pin {
                    PinMode::FactoryDefault => "123456".to_string(),
                    PinMode::Text => ask_password("Enter your PIN")?,
                    PinMode::Numpad => {
                        let mut warning = None;
                        'retry : loop {
                            let mut prompt = String::new();
                            if let Some(warning) = warning {
                                prompt.push_str(warning);
                            }
                            prompt.push_str("Press numpad buttons matching the locations of your PIN digits\n");
                            let mut digits = (1 ..= 9).collect::<Vec<_>>();
                            digits.shuffle(&mut thread_rng());
                            let digit_lookup =
                                Iterator::zip(
                                    [
                                        ['7', 'u', 'w'],
                                        ['8', 'i', 'e'],
                                        ['9', 'o', 'r'],
                                        ['4', 'j', 's'],
                                        ['5', 'k', 'f'],
                                        ['6', 'l', 'f'],
                                        ['1', 'm', 'x'],
                                        ['2', ',', 'c'],
                                        ['3', '.', 'v'],
                                    ].into_iter(),
                                    &digits,
                                )
                                    .flat_map(|(positions, digit)| positions.map(|p| (p, digit)))
                                    .collect::<HashMap<_, _>>();
                            for row in digits.chunks(3) {
                                for digit in row {
                                    prompt.push_str(&format!(" {}", digit));
                                }
                                prompt.push('\n');
                            }
                            let pre_pin = ask_password(&prompt)?;
                            let pre_pin = pre_pin.trim();
                            let mut pin = String::new();
                            for c in pre_pin.chars() {
                                let d = match digit_lookup.get(&c) {
                                    Some(d) => **d,
                                    None => {
                                        warning = Some("There were invalid digits in the PIN. Please try again.\n");
                                        continue 'retry;
                                    },
                                };
                                pin.push_str(&d.to_string());
                            }
                            break pin;
                        }
                    },
                };
                if pin.is_empty() {
                    log.log(loga::WARN, "Got empty pin, please retry");
                    sleep(Duration::from_secs(1));
                    continue;
                }
                loop {
                    let mut reader_names = pcsc_context.list_readers_owned()?.into_iter().collect::<HashSet<_>>();
                    reader_names.insert(pcsc::PNP_NOTIFICATION().to_owned());
                    let mut i = 0;
                    loop {
                        if i >= watch.len() {
                            break;
                        }
                        if reader_names.remove(&watch[i].name().to_owned()) {
                            i += 1;
                        } else {
                            watch.remove(i);
                        }
                    }
                    for new in reader_names {
                        watch.push(pcsc::ReaderState::new(new, pcsc::State::UNKNOWN));
                    }
                    log.log(loga::INFO, "Please hold your smartcard to the reader");
                    match pcsc_context.get_status_change(Duration::from_secs(10), &mut watch) {
                        Ok(_) => { },
                        Err(pcsc::Error::Timeout) => {
                            continue;
                        },
                        Err(pcsc::Error::ServiceStopped) | Err(pcsc::Error::NoService) => {
                            // Windows will kill the SmartCard service when the last reader is disconnected
                            // Restart it and wait (sleep) for a new reader connection if that occurs
                            pcsc_context = Context::establish(pcsc::Scope::User)?;
                            continue;
                        },
                        Err(err) => return Err(err.into()),
                    };
                    for state in &mut watch {
                        'detect : loop {
                            let old_state = state.current_state();
                            let new_state = state.event_state();
                            if !new_state.contains(pcsc::State::CHANGED) {
                                break 'detect;
                            }
                            if state.name() == pcsc::PNP_NOTIFICATION() {
                                break 'detect;
                            }
                            if !old_state.contains(pcsc::State::PRESENT) && new_state.contains(pcsc::State::PRESENT) {
                                match (|| {
                                    let mut card = openpgp_card::Card::new(
                                        // Workaround pending
                                        // https://gitlab.com/openpgp-card/openpgp-card/-/merge_requests/42
                                        card_backend_pcsc::PcscBackend::card_backends(None)?
                                            .next()
                                            .context("Card missing (timing?)")?
                                            .context("Error opening card backend")?,
                                    ).context("Error opening card for card backend")?;
                                    let mut tx = card.transaction().context("Error starting card transaction")?;
                                    tx.verify_user_pin(pin.clone().into()).context("Error verifying PIN")?;
                                    let card_key =
                                        CardSlot::init_from_card(
                                            &mut tx,
                                            openpgp_card::ocard::KeyType::Decryption,
                                            &|| { },
                                        ).context("Error turning card into decryption key")?;
                                    let decrypted =
                                        match card_key
                                            .decrypt_message(&encrypted)
                                            .context("Failed to decrypt disk secret - wrong key device?")? {
                                            pgp::Message::Literal(l) => l.data().to_vec(),
                                            other => {
                                                return Err(
                                                    loga::err_with(
                                                        "Rpgp returned unrecognized payload type",
                                                        ea!(other = other.dbg_str()),
                                                    ),
                                                );
                                            },
                                        };
                                    log.log(loga::INFO, "Done reading smartcard, you may now remove it");
                                    return Ok(
                                        from_utf8(decrypted)
                                            .context("Key file contains invalid utf-8")?
                                            .trim()
                                            .to_string(),
                                    );
                                })() {
                                    Ok(key) => {
                                        return Ok(key);
                                    },
                                    Err(e) => {
                                        log.log_err(loga::WARN, e.context("Failed to get volume key, retrying"));
                                    },
                                }
                            }
                            break 'detect;
                        }
                        state.sync_current_state();
                    }
                }
            }
        },
    }
}
