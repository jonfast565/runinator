#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub struct AwsChunkedDecoder {
    pub(super) state: DecodeState,
    pub(super) hasher: Sha256,
    pub(super) trailer_checksum: Option<String>,
}

impl Decoder for AwsChunkedDecoder {
    type Item = Bytes;
    type Error = std::io::Error;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            match self.state {
                DecodeState::Header => {
                    let Some(line_end) = find(source, 0, CRLF) else {
                        if source.len() > MAX_LINE_BYTES {
                            return Err(invalid("aws-chunked header is too long"));
                        }
                        return Ok(None);
                    };
                    let line = source.split_to(line_end + CRLF.len());
                    let header = std::str::from_utf8(&line[..line_end])
                        .map_err(|_| invalid("aws-chunked header is not utf-8"))?;
                    let size_text = header.split(';').next().unwrap_or("").trim();
                    let size = usize::from_str_radix(size_text, 16).map_err(|_| {
                        invalid(&format!(
                            "aws-chunked size '{size_text}' is not hexadecimal"
                        ))
                    })?;
                    self.state = if size == 0 {
                        DecodeState::Trailers
                    } else {
                        DecodeState::Data(size)
                    };
                }
                DecodeState::Data(remaining) => {
                    if remaining == 0 {
                        self.state = DecodeState::DataCrlf;
                        continue;
                    }
                    if source.is_empty() {
                        return Ok(None);
                    }
                    let read = remaining.min(source.len());
                    let data = source.split_to(read).freeze();
                    self.hasher.update(&data);
                    self.state = DecodeState::Data(remaining - read);
                    return Ok(Some(data));
                }
                DecodeState::DataCrlf => {
                    if source.len() < CRLF.len() {
                        return Ok(None);
                    }
                    if &source[..CRLF.len()] != CRLF {
                        return Err(invalid("aws-chunked data has no trailing crlf"));
                    }
                    source.advance(CRLF.len());
                    self.state = DecodeState::Header;
                }
                DecodeState::Trailers => {
                    let Some(line_end) = find(source, 0, CRLF) else {
                        if source.len() > MAX_LINE_BYTES {
                            return Err(invalid("aws-chunked trailer is too long"));
                        }
                        return Ok(None);
                    };
                    let line = source.split_to(line_end + CRLF.len());
                    if line_end == 0 {
                        if let Some(expected) = self.trailer_checksum.take() {
                            let actual = hex::encode(self.hasher.clone().finalize());
                            let expected =
                                runinator_blob_core::sha256_from_checksum_header(&expected)
                                    .ok_or_else(|| {
                                        invalid("aws-chunked checksum trailer is malformed")
                                    })?;
                            if !expected.eq_ignore_ascii_case(&actual) {
                                return Err(invalid(&format!(
                                    "aws-chunked checksum mismatch: expected {expected}, computed {actual}"
                                )));
                            }
                        }
                        self.state = DecodeState::Done;
                        continue;
                    }
                    let trailer = std::str::from_utf8(&line[..line_end])
                        .map_err(|_| invalid("aws-chunked trailer is not utf-8"))?;
                    if let Some((name, value)) = trailer.split_once(':') {
                        if name.trim().eq_ignore_ascii_case("x-amz-checksum-sha256") {
                            self.trailer_checksum = Some(value.trim().to_string());
                        }
                    }
                }
                DecodeState::Done => {
                    source.clear();
                    return Ok(None);
                }
            }
        }
    }

    fn decode_eof(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(item) = self.decode(source)? {
            return Ok(Some(item));
        }
        if matches!(self.state, DecodeState::Done) {
            return Ok(None);
        }
        Err(invalid("aws-chunked body ended before its final trailer"))
    }
}
