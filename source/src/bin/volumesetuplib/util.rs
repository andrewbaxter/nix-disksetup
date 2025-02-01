use {
    loga::{
        ea,
        DebugDisplay,
        Log,
        ResultContext,
    },
    serde::de::DeserializeOwned,
    std::{
        io::Write,
        process::{
            Command,
        },
    },
};

pub(crate) fn from_utf8(data: Vec<u8>) -> Result<String, loga::Error> {
    return Ok(
        String::from_utf8(
            data,
        ).map_err(
            |e| loga::err_with(
                "Received bytes are not valid utf-8",
                ea!(bytes = String::from_utf8_lossy(&e.as_bytes())),
            ),
        )?,
    );
}

pub(crate) struct SimpleCommand<'a>(&'a mut Command);

impl<'a> SimpleCommand<'a> {
    pub(crate) fn run(&mut self) -> Result<(), loga::Error> {
        let log = Log::new().fork(ea!(command = self.0.dbg_str()));
        self.0.stdout(std::process::Stdio::piped());
        self.0.stderr(std::process::Stdio::piped());
        let o = self.0.output().stack_context(&log, "Failed to start child process")?;
        if !o.status.success() {
            return Err(
                log.err_with(
                    "Child process exited with error",
                    ea!(code = o.status.code().dbg_str(), output = o.dbg_str()),
                ),
            );
        }
        return Ok(());
    }

    pub(crate) fn run_stdin(&mut self, data: &[u8]) -> Result<(), loga::Error> {
        let log = Log::new().fork(ea!(command = self.0.dbg_str()));
        self.0.stdout(std::process::Stdio::piped());
        self.0.stderr(std::process::Stdio::piped());
        self.0.stdin(std::process::Stdio::piped());
        let mut child = self.0.spawn().stack_context(&log, "Failed to start child process")?;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(data).stack_context(&log, "Error writing to child process stdin")?;
        let output = child.wait_with_output().stack_context(&log, "Failed to wait for child process to exit")?;
        if !output.status.success() {
            return Err(
                log.err_with(
                    "Child process exited with error",
                    ea!(code = output.status.code().dbg_str(), output = output.dbg_str()),
                ),
            );
        }
        return Ok(());
    }

    pub(crate) fn run_stdout(&mut self) -> Result<Vec<u8>, loga::Error> {
        let log = Log::new().fork(ea!(command = self.0.dbg_str()));
        self.0.stdout(std::process::Stdio::piped());
        self.0.stderr(std::process::Stdio::piped());
        let child = self.0.spawn().stack_context(&log, "Failed to start child process")?;
        let output = child.wait_with_output().stack_context(&log, "Failed to wait for child process to exit")?;
        if !output.status.success() {
            return Err(
                log.err_with(
                    "Child process exited with error",
                    ea!(code = output.status.code().dbg_str(), output = output.dbg_str()),
                ),
            );
        }
        return Ok(output.stdout);
    }

    pub(crate) fn run_json_out<D: DeserializeOwned>(&mut self) -> Result<D, loga::Error> {
        let res = self.run_stdout()?;
        let log = Log::new().fork(ea!(command = self.0.dbg_str()));
        return Ok(
            serde_json::from_slice(
                &res,
            ).stack_context_with(&log, "Error parsing output as json", ea!(output = res.dbg_str()))?,
        );
    }
}

pub(crate) trait SimpleCommandExt {
    fn simple<'a>(&'a mut self) -> SimpleCommand<'a>;
}

impl SimpleCommandExt for Command {
    fn simple<'a>(&'a mut self) -> SimpleCommand<'a> {
        return SimpleCommand(self);
    }
}
