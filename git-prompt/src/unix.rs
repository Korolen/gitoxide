/// The path to the default TTY on linux
pub const TTY_PATH: &str = "/dev/tty";

#[cfg(unix)]
pub(crate) mod imp {
    use crate::unix::TTY_PATH;
    use crate::{Error, Options};
    use nix::sys::termios;
    use nix::sys::termios::Termios;
    use parking_lot::lock_api::MutexGuard;
    use parking_lot::{Mutex, RawMutex};
    use std::io::{BufRead, Write};
    use std::os::unix::io::{AsRawFd, RawFd};

    static TERM_STATE: Mutex<Option<Termios>> = Mutex::new(None);

    /// Ask the user given a `prompt`, returning the result.
    pub fn ask(prompt: &str, Options { secret }: Options) -> Result<String, Error> {
        if secret {
            let state = TERM_STATE.lock();
            let mut in_out = std::fs::OpenOptions::new().write(true).read(true).open(TTY_PATH)?;
            let restore = save_term_state_and_disable_echo(state, in_out.as_raw_fd())?;
            in_out.write_all(prompt.as_bytes())?;

            let mut buf_read = std::io::BufReader::with_capacity(64, in_out);
            let mut out = String::with_capacity(64);
            buf_read.read_line(&mut out)?;

            out.pop();
            if out.ends_with("\r") {
                out.pop();
            }
            restore.now()?;
            Ok(out)
        } else {
            let mut in_out = std::fs::OpenOptions::new().write(true).read(true).open(TTY_PATH)?;
            in_out.write_all(prompt.as_bytes())?;

            let mut buf_read = std::io::BufReader::with_capacity(64, in_out);
            let mut out = String::with_capacity(64);
            buf_read.read_line(&mut out)?;
            Ok(out.trim_end().to_owned())
        }
    }

    type TermiosGuard<'a> = MutexGuard<'a, RawMutex, Option<Termios>>;

    struct RestoreTerminalStateOnDrop<'a> {
        state: TermiosGuard<'a>,
        fd: RawFd,
    }

    impl<'a> RestoreTerminalStateOnDrop<'a> {
        fn now(mut self) -> Result<(), Error> {
            let state = self.state.take().expect("BUG: we exist only if something is saved");
            termios::tcsetattr(self.fd, termios::SetArg::TCSAFLUSH, &state)?;
            Ok(())
        }
    }

    impl<'a> Drop for RestoreTerminalStateOnDrop<'a> {
        fn drop(&mut self) {
            if let Some(state) = self.state.take() {
                termios::tcsetattr(self.fd, termios::SetArg::TCSAFLUSH, &state).ok();
            }
        }
    }

    fn save_term_state_and_disable_echo(
        mut state: TermiosGuard<'_>,
        fd: RawFd,
    ) -> Result<RestoreTerminalStateOnDrop<'_>, Error> {
        assert!(
            state.is_none(),
            "BUG: recursive calls are not possible and we restore afterwards"
        );

        let prev = termios::tcgetattr(fd)?;
        let mut new = prev.clone();
        *state = prev.into();

        new.local_flags &= !termios::LocalFlags::ECHO;
        new.local_flags |= termios::LocalFlags::ECHONL;
        termios::tcsetattr(fd, termios::SetArg::TCSAFLUSH, &new)?;

        Ok(RestoreTerminalStateOnDrop { fd, state })
    }
}