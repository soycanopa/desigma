//! Tolerant JSON: pen.dev accepts `//` and `/* */` comments, so the text is
//! cleaned (comments blanked, trailing commas dropped) before `serde_json`.

/// Returns `text` without comments and trailing commas. Strings are left
/// untouched, and every removed character becomes a space (newlines stay) so
/// parse errors keep their line and column.
pub fn clean(text: &str) -> String {
    let b = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                out.push(b'"');
                i += 1;
                while i < b.len() {
                    out.push(b[i]);
                    match b[i] {
                        b'\\' if i + 1 < b.len() => {
                            out.push(b[i + 1]);
                            i += 2;
                            continue;
                        }
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    out.push(b' ');
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                out.extend_from_slice(b"  ");
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    out.push(if b[i] == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
                if i < b.len() {
                    out.extend_from_slice(b"  ");
                    i += 2;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    drop_trailing_commas(&mut out);
    // Only ASCII bytes were replaced, so UTF-8 sequences are intact.
    String::from_utf8(out).unwrap_or_default()
}

fn drop_trailing_commas(b: &mut [u8]) {
    let mut in_str = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b',' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && (b[j] == b'}' || b[j] == b']') {
                b[i] = b' ';
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> serde_json::Value {
        serde_json::from_str(&clean(s)).unwrap()
    }

    #[test]
    fn strips_line_and_block_comments() {
        let v = parse("{ // note\n \"a\": 1, /* two\nlines */ \"b\": [2, 3] }");
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"][1], 3);
    }

    #[test]
    fn leaves_comment_markers_inside_strings() {
        let v = parse(r#"{"u": "http://x/*y*/", "e": "a\"//b"}"#);
        assert_eq!(v["u"], "http://x/*y*/");
        assert_eq!(v["e"], "a\"//b");
    }

    #[test]
    fn drops_trailing_commas() {
        let v = parse("{\"a\": [1, 2,], \"b\": {\"c\": 1,},}");
        assert_eq!(v["a"].as_array().unwrap().len(), 2);
        assert_eq!(v["b"]["c"], 1);
    }

    #[test]
    fn keeps_non_ascii_text() {
        assert_eq!(parse("{\"t\": \"héllo // ✓\"} // end")["t"], "héllo // ✓");
    }

    #[test]
    fn unterminated_block_comment_does_not_panic() {
        let _ = clean("{\"a\": 1 /* never closed");
        let _ = clean("\"open string");
    }
}
