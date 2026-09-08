use super::ApiError;

#[derive(Default)]
pub struct EventDecoder {
    line: Vec<u8>,
    data: String,
    first_line: bool,
}

impl EventDecoder {
    pub fn new() -> Self {
        Self {
            first_line: true,
            ..Self::default()
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, ApiError> {
        let mut events = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                self.finish_line(&mut events)?;
            } else {
                self.line.push(byte);
            }
            if self.line.len() + self.data.len() > 1_048_576 {
                return Err(ApiError::InvalidResponse);
            }
        }
        Ok(events)
    }

    fn finish_line(&mut self, events: &mut Vec<String>) -> Result<(), ApiError> {
        let bytes = std::mem::take(&mut self.line);
        let line = std::str::from_utf8(&bytes).map_err(|_| ApiError::InvalidResponse)?;
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line = if std::mem::take(&mut self.first_line) {
            line.trim_start_matches('\u{feff}')
        } else {
            line
        };
        if line.is_empty() {
            if !self.data.is_empty() {
                self.data.pop();
                events.push(std::mem::take(&mut self.data));
            }
        } else if let Some(value) = line.strip_prefix("data:") {
            self.data.push_str(value.strip_prefix(' ').unwrap_or(value));
            self.data.push('\n');
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_utf8_across_every_byte_boundary() {
        let wire = "\u{feff}: heartbeat\r\ndata: 日本語\r\ndata: second\r\n\r\ndata: [DONE]\n\n";
        let mut decoder = EventDecoder::new();
        let mut events = Vec::new();
        for byte in wire.as_bytes() {
            events.extend(decoder.push(&[*byte]).unwrap());
        }
        assert_eq!(events, ["日本語\nsecond", "[DONE]"]);
    }

    #[test]
    fn ignores_comments_and_does_not_dispatch_incomplete_events() {
        let mut decoder = EventDecoder::new();
        assert!(
            decoder
                .push(b": ping\n\ndata: partial\n")
                .unwrap()
                .is_empty()
        );
        assert_eq!(decoder.push(b"\n").unwrap(), ["partial"]);
    }

    #[test]
    fn rejects_unbounded_events() {
        assert!(EventDecoder::new().push(&vec![b'x'; 1_048_577]).is_err());
    }
}
