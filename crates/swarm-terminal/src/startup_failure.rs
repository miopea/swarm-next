//! Narrow, bounded observation of the actual interactive startup byte stream.
//! Never searches a reconstructed screen or a previous conversation transcript.

const MAX_STARTUP_BYTES: usize = 4096;
const MISSING_CONTINUATION: &[u8] = b"No conversation found to continue";

pub(crate) struct StartupFailureCapture {
    bytes: Option<Vec<u8>>,
    missing: bool,
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
}

impl StartupFailureCapture {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            bytes: enabled.then(Vec::new),
            missing: false,
            engaged: false,
            failed_at_startup: false,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.bytes = None;
        self.missing = false;
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

    pub(crate) fn missing_continuation(&self) -> bool {
        self.missing
    }

    pub(crate) fn finish(&mut self, complete: bool) {
        let bytes = self.bytes.take();
        // `missing` stays exact: it is what TELLS the operator the provider had
        // no conversation, and a loose match there would put words in the
        // provider's mouth. Recovery no longer depends on it.
        self.missing = complete && bytes.as_deref().is_some_and(exact_missing_message);
        self.failed_at_startup = complete && !self.engaged;
    }
}

fn exact_missing_message(mut bytes: &[u8]) -> bool {
    let mut matched = 0;
    while let Some((&byte, rest)) = bytes.split_first() {
        bytes = rest;
        if byte == 0x1b {
            let Some(csi) = bytes.strip_prefix(b"[") else {
                return false;
            };
            // Only styling and cursor visibility are ignorable. Cursor movement,
            // erase, OSC and unknown controls could rewrite or hide other text.
            let Some(end) = csi
                .iter()
                .take(32)
                .position(|byte| (0x40..=0x7e).contains(byte))
            else {
                return false;
            };
            let sequence = &csi[..=end];
            let sgr = sequence[end] == b'm'
                && sequence[..end]
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || *byte == b';');
            if !sgr && sequence != b"?25h" && sequence != b"?25l" {
                return false;
            }
            bytes = &csi[end + 1..];
        } else {
            // Allow line framing, but never erase internal whitespace or prose.
            let framing = matches!(byte, b' ' | b'\r' | b'\n');
            if framing && (matched == 0 || matched == MISSING_CONTINUATION.len()) {
                continue;
            }
            if MISSING_CONTINUATION.get(matched) != Some(&byte) {
                return false;
            }
            matched += 1;
        }
    }
    matched == MISSING_CONTINUATION.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_error_survives_chunk_boundaries_and_limited_styling() {
        for size in 1..=64 {
            let mut capture = StartupFailureCapture::new(true);
            for chunk in b"\r\n\x1b[0m\x1b[31mNo conversation found to continue\x1b[0m\r\n\x1b[?25h"
                .chunks(size)
            {
                capture.push(chunk);
            }
            assert!(!capture.missing_continuation(), "reader has not finished");
            capture.finish(true);
            assert!(capture.bytes.is_none());
            assert!(capture.missing_continuation(), "chunk size {size}");
        }
    }

    #[test]
    fn other_output_and_screen_rewriting_never_prove_absence() {
        for bytes in [
            b"".as_slice(),
            b"Error: Invalid MCP configuration",
            b"No conversation found to continue later",
            b"Previous output: No conversation found to continue",
            b"No conversation found to continue\nOther output",
            b"No conversation found to continue\x1b[2J",
            b"No conversation found to continue\x1b[1G",
            b"No conversation found to continue\x1b]0;title\x07",
            b"No conversation found to continue\x1b[31",
            b"No conversation found to continue\x00",
            b"No conversation found to continue\x0b",
        ] {
            let mut capture = StartupFailureCapture::new(true);
            capture.push(bytes);
            capture.finish(true);
            assert!(!capture.missing_continuation());
        }
    }

    #[test]
    fn overflow_and_disarming_are_permanent_and_release_bytes() {
        let mut capture = StartupFailureCapture::new(true);
        capture.push(&vec![b' '; MAX_STARTUP_BYTES]);
        capture.push(MISSING_CONTINUATION);
        assert!(capture.bytes.is_none());
        assert!(!capture.missing_continuation());
        for enabled in [true, false] {
            let mut capture = StartupFailureCapture::new(enabled);
            capture.push(MISSING_CONTINUATION);
            capture.disarm();
            capture.push(MISSING_CONTINUATION);
            assert!(capture.bytes.is_none());
            assert!(!capture.missing_continuation());
        }
    }

    #[test]
    fn incomplete_stream_cannot_confirm_the_exact_message() {
        let mut capture = StartupFailureCapture::new(true);
        capture.push(MISSING_CONTINUATION);
        capture.finish(false);
        assert!(capture.bytes.is_none());
        assert!(!capture.missing_continuation());
    }
}
