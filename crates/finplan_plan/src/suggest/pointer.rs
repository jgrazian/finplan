//! RFC 6901 pointers and the three RFC 6902 operations a `Change` can make.
//!
//! Deliberately small: `serde_json::Value::pointer` covers reads, but not the
//! insert-into-array and append (`-`) forms `add` needs, nor removal.

use serde_json::Value;

/// A parsed pointer: the reference tokens, unescaped. Empty is the whole
/// document.
pub(crate) fn parse(pointer: &str) -> Result<Vec<String>, String> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    let Some(rest) = pointer.strip_prefix('/') else {
        return Err("a path must be empty or start with '/'".into());
    };
    rest.split('/')
        .map(|token| {
            if token.contains('~') && !valid_escapes(token) {
                return Err(format!("'{token}' has a '~' that is not '~0' or '~1'"));
            }
            Ok(token.replace("~1", "/").replace("~0", "~"))
        })
        .collect()
}

fn valid_escapes(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'~')
        .all(|(i, _)| matches!(bytes.get(i + 1), Some(b'0' | b'1')))
}

/// The value `tokens` names, if there is one.
pub(crate) fn get<'a>(root: &'a Value, tokens: &[String]) -> Option<&'a Value> {
    tokens.iter().try_fold(root, |node, token| match node {
        Value::Object(map) => map.get(token),
        Value::Array(items) => index(token, items.len()).and_then(|i| items.get(i)),
        _ => None,
    })
}

fn get_mut<'a>(root: &'a mut Value, tokens: &[String]) -> Option<&'a mut Value> {
    tokens.iter().try_fold(root, |node, token| match node {
        Value::Object(map) => map.get_mut(token),
        Value::Array(items) => {
            let len = items.len();
            index(token, len).and_then(|i| items.get_mut(i))
        }
        _ => None,
    })
}

/// An array index token: decimal, no leading zeros, below `len`.
fn index(token: &str, len: usize) -> Option<usize> {
    if token.is_empty() || (token.len() > 1 && token.starts_with('0')) {
        return None;
    }
    token.parse::<usize>().ok().filter(|i| *i < len)
}

/// Split into (parent tokens, last token). Callers handle the root themselves.
fn split(tokens: &[String]) -> (&[String], &str) {
    let (last, parent) = tokens.split_last().expect("root is handled by the caller");
    (parent, last)
}

pub(crate) fn replace(root: &mut Value, tokens: &[String], value: Value) -> Result<(), String> {
    let slot = get_mut(root, tokens).ok_or("nothing at this path to replace")?;
    *slot = value;
    Ok(())
}

pub(crate) fn add(root: &mut Value, tokens: &[String], value: Value) -> Result<(), String> {
    let (parent, last) = split(tokens);
    match get_mut(root, parent).ok_or("the parent of this path does not exist")? {
        Value::Object(map) => {
            map.insert(last.to_string(), value);
            Ok(())
        }
        Value::Array(items) => {
            if last == "-" {
                items.push(value);
                return Ok(());
            }
            // Insertion may name one past the end, which appends.
            let at = index(last, items.len() + 1)
                .ok_or_else(|| format!("'{last}' is not an index into this list"))?;
            items.insert(at, value);
            Ok(())
        }
        _ => Err("the parent of this path is not an object or a list".into()),
    }
}

pub(crate) fn remove(root: &mut Value, tokens: &[String]) -> Result<Value, String> {
    let (parent, last) = split(tokens);
    match get_mut(root, parent).ok_or("the parent of this path does not exist")? {
        Value::Object(map) => map
            .remove(last)
            .ok_or_else(|| format!("no field '{last}' to remove")),
        Value::Array(items) => {
            let at = index(last, items.len())
                .ok_or_else(|| format!("'{last}' is not an index into this list"))?;
            Ok(items.remove(at))
        }
        _ => Err("the parent of this path is not an object or a list".into()),
    }
}

/// Deep equality, reading numbers as equal within a small relative tolerance:
/// a suggestion quotes the value it read back through JSON, and `0.1 + 0.2`
/// style noise must not make it look stale.
pub(crate) fn approx_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => {
                let scale = x.abs().max(y.abs()).max(1.0);
                (x - y).abs() <= 1e-6 * scale
            }
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| approx_eq(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| approx_eq(v, w)))
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn toks(p: &str) -> Vec<String> {
        parse(p).unwrap()
    }

    #[test]
    fn parses_escapes() {
        assert_eq!(toks(""), Vec::<String>::new());
        assert_eq!(toks("/a~1b/c~0d"), vec!["a/b", "c~d"]);
        assert!(parse("a").is_err());
        assert!(parse("/a~2").is_err());
    }

    #[test]
    fn add_replace_remove() {
        let mut doc = json!({"list": [1, 2], "obj": {"k": 1}});
        add(&mut doc, &toks("/list/-"), json!(3)).unwrap();
        add(&mut doc, &toks("/list/0"), json!(0)).unwrap();
        replace(&mut doc, &toks("/obj/k"), json!(2)).unwrap();
        assert_eq!(remove(&mut doc, &toks("/list/1")).unwrap(), json!(1));
        assert_eq!(doc, json!({"list": [0, 2, 3], "obj": {"k": 2}}));
        assert!(replace(&mut doc, &toks("/obj/missing"), json!(1)).is_err());
        assert!(remove(&mut doc, &toks("/list/01")).is_err());
        assert!(add(&mut doc, &toks("/list/9"), json!(1)).is_err());
    }

    #[test]
    fn approximate_numbers() {
        assert!(approx_eq(&json!({"v": 0.3}), &json!({"v": 0.1 + 0.2})));
        assert!(approx_eq(&json!(20000), &json!(20000.0)));
        assert!(!approx_eq(&json!(20000), &json!(20001)));
    }
}
