//! Narrow, bounded observation of the actual interactive startup byte stream.
//! Never searches a reconstructed screen or a previous conversation transcript.

const MAX_STARTUP_BYTES: usize = 4096;

pub(crate) struct StartupFailureCapture {
    bytes: Option<Vec<u8>>,
    /// Whether anything happened that makes this session the operator's.
    ///
    /// ⚠️ KEPT SEPARATE FROM `missing` DELIBERATELY. Both used to be cleared by
    /// the same `disarm`, so one flag was carrying two unrelated facts: "the
    /// provider's exact words were not seen" and "a human has engaged with this
    /// terminal". Recovery must ignore the first and must ALWAYS respect the
    /// second — typing into a session makes it yours, and no fallback may
    /// replace it afterwards.
    engaged: bool,
    /// Whether the startup stream ran to completion without being disarmed.
    failed_at_startup: bool,
    /// Whether this session is observed for startup failure at all.
    ///
    /// ⚠️ NOT DERIVABLE FROM `bytes`. An empty buffer means "armed and nothing
    /// captured"; a missing one means "never armed" OR "disarmed". While the
    /// only question asked here was the exact message, an unarmed capture could
    /// never answer yes and the difference did not matter. The structural
    /// question CAN answer yes without reading a byte, so a scratch shell — no
    /// provider, never observed — started reporting that its startup failed.
    armed: bool,
}

impl StartupFailureCapture {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            bytes: enabled.then(Vec::new),
            engaged: false,
            failed_at_startup: false,
            armed: enabled,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.bytes = None;
        self.engaged = true;
        self.failed_at_startup = false;
    }

    /// Whether this session died during startup having never been engaged with.
    ///
    /// The structural question recovery actually needs, as opposed to whether
    /// the provider's wording matched byte for byte.
    pub(crate) fn failed_before_anyone_used_it(&self) -> bool {
        self.failed_at_startup && !self.engaged
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let Some(buffer) = &mut self.bytes else {
            return;
        };
        if bytes.len() > MAX_STARTUP_BYTES.saturating_sub(buffer.len()) {
            self.disarm();
        } else {
            buffer.extend_from_slice(bytes);
        }
    }

    /// ⚠️ THE CAPTURED BYTES ARE DROPPED WITHOUT BEING READ, and that is the
    /// point. Until 1.13.2 they were matched against the provider's exact
    /// refusal, and that match gated recovery — so a prefix, a full stop or a
    /// second line meant a worker that could not start was never relaunched.
    /// The provider owns its wording and may change it in any release; what
    /// Swarm can actually stand on is that the startup ran to completion,
    /// nobody engaged with it, and the process died. The buffer stays because
    /// bounding and releasing it is what keeps a runaway startup from growing
    /// without limit.
    pub(crate) fn finish(&mut self, complete: bool) {
        self.bytes = None;
        self.failed_at_startup = self.armed && complete && !self.engaged;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_completed_unengaged_startup_is_a_failure_whatever_it_printed() {
        for bytes in [
            b"".as_slice(),
            b"No conversation found to continue",
            b"Old transcript: No conversation found to continue",
            b"No conversation found to continue.\r\n",
            b"Error: Invalid MCP configuration",
        ] {
            let mut capture = StartupFailureCapture::new(true);
            capture.push(bytes);
            assert!(
                !capture.failed_before_anyone_used_it(),
                "the reader has not finished"
            );
            capture.finish(true);
            assert!(capture.bytes.is_none(), "the buffer is released");
            assert!(capture.failed_before_anyone_used_it());
        }
    }

    #[test]
    fn an_unarmed_capture_never_reports_a_startup_failure() {
        // A scratch shell is not observed for startup failure. Without this the
        // structural flag answers yes for a session nothing ever watched, which
        // the exact-message question could not do because it had no bytes to
        // match — so the leak arrived with the change of question.
        let mut capture = StartupFailureCapture::new(false);
        capture.push(b"anything at all");
        capture.finish(true);
        assert!(!capture.failed_before_anyone_used_it());
    }

    #[test]
    fn engagement_and_overflow_permanently_disarm_and_release_bytes() {
        let mut capture = StartupFailureCapture::new(true);
        capture.push(&vec![b' '; MAX_STARTUP_BYTES]);
        capture.push(b"No conversation found to continue");
        assert!(capture.bytes.is_none());
        capture.finish(true);
        assert!(!capture.failed_before_anyone_used_it());

        // Typing into a terminal makes it yours, and no fallback may replace it.
        for enabled in [true, false] {
            let mut capture = StartupFailureCapture::new(enabled);
            capture.push(b"No conversation found to continue");
            capture.disarm();
            capture.push(b"No conversation found to continue");
            capture.finish(true);
            assert!(capture.bytes.is_none());
            assert!(!capture.failed_before_anyone_used_it());
        }
    }

    #[test]
    fn an_incomplete_stream_cannot_confirm_a_startup_failure() {
        let mut capture = StartupFailureCapture::new(true);
        capture.push(b"No conversation found to continue");
        capture.finish(false);
        assert!(capture.bytes.is_none());
        assert!(!capture.failed_before_anyone_used_it());
    }
}
