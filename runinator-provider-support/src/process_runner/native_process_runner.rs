#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Default)]
pub struct NativeProcessRunner;

impl ProcessRunner for NativeProcessRunner {
    fn run(&self, request: ProcessRequest<'_>) -> Result<ProcessResult, ProcessFailure> {
        if request.cancellation.is_cancelled() {
            return Err(ProcessFailure::Canceled);
        }
        let started = Instant::now();
        let mut child = request
            .command
            .stdin(if request.input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ProcessFailure::Spawn)?;
        let pump = match ProcessOutputPump::start(&mut child, request.sink) {
            Ok(pump) => pump,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProcessFailure::Io(error));
            }
        };
        // writing input concurrently lets cancellation and deadlines interrupt a blocked pipe.
        let writer = child
            .stdin
            .take()
            .zip(request.input)
            .map(|(mut stdin, input)| thread::spawn(move || stdin.write_all(&input)));
        let status = loop {
            if request.cancellation.is_cancelled() {
                break Err(ProcessFailure::Canceled);
            }
            if started.elapsed() >= request.timeout {
                break Err(ProcessFailure::TimedOut);
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => break Err(ProcessFailure::Io(error)),
            }
        };
        if status.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let input_result = writer
            .map(|writer| {
                writer
                    .join()
                    .unwrap_or_else(|_| Err(io::Error::other("process input writer panicked")))
            })
            .transpose();
        let output = pump.finish();
        let status = status?;
        input_result.map_err(ProcessFailure::Io)?;
        Ok(ProcessResult { status, output })
    }
}
