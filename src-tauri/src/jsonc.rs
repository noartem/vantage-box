//! Минимальный препроцессор JSONC → JSON.
//!
//! Вырезает `//`- и `/* */`-комментарии и висячие запятые, заменяя их пробелами,
//! чтобы смещения символов не съезжали — тогда номера строк в ошибках serde_json
//! указывают на реальное место в исходном файле.

/// Приводит JSONC к валидному JSON той же длины.
pub fn strip_jsonc(input: &str) -> String {
    let without_comments = strip_comments(input);
    strip_trailing_commas(&without_comments)
}

fn strip_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                // Строковый литерал копируем как есть, уважая экранирование.
                out.push(bytes[i]);
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    out.push(b);
                    i += 1;
                    if b == b'\\' {
                        if i < bytes.len() {
                            out.push(bytes[i]);
                            i += 1;
                        }
                    } else if b == b'"' {
                        break;
                    }
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out.push(b' ');
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                out.push(b' ');
                out.push(b' ');
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                        out.push(b' ');
                        out.push(b' ');
                        i += 2;
                        break;
                    }
                    // Переводы строк сохраняем, остальное превращаем в пробел.
                    out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }

    // Мы работали побайтово, но резали только ASCII-последовательности вне строк,
    // поэтому UTF-8 остался целым.
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

fn strip_trailing_commas(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    i += 1;
                    if b == b'\\' {
                        i += 1;
                    } else if b == b'"' {
                        break;
                    }
                }
            }
            b',' => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'}' || bytes[j] == b']') {
                    out[i] = b' ';
                }
                i += 1;
            }
            _ => i += 1,
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

#[cfg(test)]
mod tests {
    use super::strip_jsonc;

    #[test]
    fn keeps_length_and_removes_line_comments() {
        let src = "{\n  \"a\": 1 // hello\n}";
        let out = strip_jsonc(src);
        assert_eq!(out.len(), src.len());
        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap()["a"], 1);
    }

    #[test]
    fn removes_block_comments() {
        let src = "{/* note\n   more */ \"a\": 1}";
        let out = strip_jsonc(src);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap()["a"], 1);
    }

    #[test]
    fn does_not_touch_comment_like_strings() {
        let src = r#"{"url": "http://x/y", "p": "a/*b*/c"}"#;
        let out = strip_jsonc(src);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["url"], "http://x/y");
        assert_eq!(v["p"], "a/*b*/c");
    }

    #[test]
    fn removes_trailing_commas() {
        let src = "{\n  \"a\": [1, 2, ],\n}";
        let out = strip_jsonc(src);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["a"][1], 2);
    }

    #[test]
    fn survives_escaped_quotes_and_utf8() {
        let src = r#"{"s": "он сказал \"привет\" // не комментарий"}"#;
        let out = strip_jsonc(src);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["s"], "он сказал \"привет\" // не комментарий");
    }
}
