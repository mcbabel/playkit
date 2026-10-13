use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Obj(Obj),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Obj(pub Vec<(String, Value)>);

impl Obj {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            Value::Obj(_) => None,
        }
    }

    pub fn obj(&self, key: &str) -> Option<&Obj> {
        match self.get(key)? {
            Value::Obj(o) => Some(o),
            Value::Str(_) => None,
        }
    }

    pub fn path(&self, keys: &[&str]) -> Option<&Obj> {
        keys.iter().try_fold(self, |o, k| o.obj(k))
    }

    pub fn obj_mut_or_insert(&mut self, key: &str) -> &mut Obj {
        let pos = self
            .0
            .iter()
            .position(|(k, v)| k.eq_ignore_ascii_case(key) && matches!(v, Value::Obj(_)));
        let i = match pos {
            Some(i) => i,
            None => {
                self.0.push((key.to_string(), Value::Obj(Obj::default())));
                self.0.len() - 1
            }
        };
        match &mut self.0[i].1 {
            Value::Obj(o) => o,
            Value::Str(_) => unreachable!(),
        }
    }

    pub fn set_str(&mut self, key: &str, value: &str) {
        match self.get_mut(key) {
            Some(v) => *v = Value::Str(value.to_string()),
            None => self
                .0
                .push((key.to_string(), Value::Str(value.to_string()))),
        }
    }
}

pub fn parse(src: &str) -> Result<Obj> {
    let mut p = Parser {
        chars: src.chars().collect(),
        pos: 0,
    };
    let obj = p.object(true)?;
    Ok(obj)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn skip(&mut self) {
        while let Some(&c) = self.chars.get(self.pos) {
            if c.is_whitespace() {
                self.pos += 1;
            } else if c == '/' && self.chars.get(self.pos + 1) == Some(&'/') {
                while self.chars.get(self.pos).is_some_and(|&c| c != '\n') {
                    self.pos += 1;
                }
            } else if c == '[' {
                while self.chars.get(self.pos).is_some_and(|&c| c != ']') {
                    self.pos += 1;
                }
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn token(&mut self) -> Result<Option<String>> {
        self.skip();
        let Some(&c) = self.chars.get(self.pos) else {
            return Ok(None);
        };
        if c == '{' || c == '}' {
            return Ok(None);
        }
        let mut out = String::new();
        if c == '"' {
            self.pos += 1;
            loop {
                let Some(&c) = self.chars.get(self.pos) else {
                    return Err(Error::Vdf("unterminated string".into()));
                };
                self.pos += 1;
                match c {
                    '"' => break,
                    '\\' => {
                        let e = self.chars.get(self.pos).copied().unwrap_or('\\');
                        self.pos += 1;
                        out.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    }
                    other => out.push(other),
                }
            }
        } else {
            while let Some(&c) = self.chars.get(self.pos) {
                if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                    break;
                }
                out.push(c);
                self.pos += 1;
            }
        }
        Ok(Some(out))
    }

    fn object(&mut self, top: bool) -> Result<Obj> {
        let mut obj = Obj::default();
        loop {
            self.skip();
            match self.chars.get(self.pos) {
                None if top => return Ok(obj),
                None => return Err(Error::Vdf("unexpected end".into())),
                Some('}') if !top => {
                    self.pos += 1;
                    return Ok(obj);
                }
                Some('}') => return Err(Error::Vdf("unexpected }".into())),
                _ => {}
            }
            let key = self
                .token()?
                .ok_or_else(|| Error::Vdf("expected key".into()))?;
            self.skip();
            if self.chars.get(self.pos) == Some(&'{') {
                self.pos += 1;
                obj.0.push((key, Value::Obj(self.object(false)?)));
            } else {
                let value = self
                    .token()?
                    .ok_or_else(|| Error::Vdf(format!("expected value for {key}")))?;
                obj.0.push((key, Value::Str(value)));
            }
        }
    }
}

pub fn write(obj: &Obj) -> String {
    let mut out = String::new();
    write_obj(obj, 0, &mut out);
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_obj(obj: &Obj, depth: usize, out: &mut String) {
    let indent = "\t".repeat(depth);
    for (k, v) in &obj.0 {
        match v {
            Value::Str(s) => {
                out.push_str(&format!("{indent}\"{}\"\t\t\"{}\"\n", escape(k), escape(s)))
            }
            Value::Obj(o) => {
                out.push_str(&format!("{indent}\"{}\"\n{indent}{{\n", escape(k)));
                write_obj(o, depth + 1, out);
                out.push_str(&format!("{indent}}}\n"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
"UserLocalConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"apps"
				{
					"1961460"
					{
						"LaunchOptions"		"WINEDLLOVERRIDES=\"dxgi=n,b\" %command%"
					}
				}
			}
		}
	}
}
"#;

    #[test]
    fn roundtrip_keeps_escapes() {
        let o = parse(SAMPLE).unwrap();
        let app = o
            .path(&[
                "UserLocalConfigStore",
                "Software",
                "Valve",
                "Steam",
                "apps",
                "1961460",
            ])
            .unwrap();
        assert_eq!(
            app.str("LaunchOptions"),
            Some("WINEDLLOVERRIDES=\"dxgi=n,b\" %command%")
        );
        assert_eq!(parse(&write(&o)).unwrap(), o);
    }

    #[test]
    fn case_insensitive_lookup() {
        let o = parse(SAMPLE).unwrap();
        assert!(o
            .path(&["userlocalconfigstore", "software", "valve", "steam", "Apps"])
            .is_some());
    }
}
