//! Text KeyValues: `.vdf`, `.acf`, gameinfo.txt.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
	String(String),
	Object(Vec<Node>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
	pub key: String,
	pub value: Value,
}

impl Node {
	/// Keys are compared case-insensitively, as Steam and the engine do.
	pub fn find(&self, name: &str) -> Option<&Node> {
		self.children()
			.iter()
			.find(|child| child.key.eq_ignore_ascii_case(name))
	}

	/// The string value of the child `name`; `None` if it is missing or an object.
	pub fn get(&self, name: &str) -> Option<&str> {
		self.find(name).and_then(Node::as_str)
	}

	pub fn as_str(&self) -> Option<&str> {
		match &self.value {
			Value::String(s) => Some(s),
			Value::Object(_) => None,
		}
	}

	pub fn is_object(&self) -> bool {
		matches!(self.value, Value::Object(_))
	}

	/// Empty for a string value.
	pub fn children(&self) -> &[Node] {
		match &self.value {
			Value::Object(children) => children,
			Value::String(_) => &[],
		}
	}
}

#[derive(Debug, Clone, Copy)]
pub struct ParseOptions {
	/// Steam's own files escape backslashes; game files such as gameinfo.txt do not.
	pub escapes: bool,
}

impl Default for ParseOptions {
	fn default() -> Self {
		Self { escapes: true }
	}
}

impl ParseOptions {
	pub const RAW: Self = Self { escapes: false };
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FileError {
	#[error(transparent)]
	Io(#[from] std::io::Error),
	#[error(transparent)]
	Parse(#[from] ParseError),
}

/// The root has an empty key and the top-level pairs as children.
pub fn parse(text: &str, options: ParseOptions) -> Result<Node, ParseError> {
	let mut parser = Parser {
		text: text.strip_prefix('\u{feff}').unwrap_or(text),
		pos: 0,
		options,
	};
	let children = parser.object(true)?;
	Ok(Node {
		key: String::new(),
		value: Value::Object(children),
	})
}

pub fn parse_file(path: &Path, options: ParseOptions) -> Result<Node, FileError> {
	let bytes = std::fs::read(path)?;
	Ok(parse(&String::from_utf8_lossy(&bytes), options)?)
}

enum Token {
	String { text: String, quoted: bool },
	Open,
	Close,
	End,
}

impl Token {
	/// Platform conditionals such as `[$WIN32]` carry no data we need.
	fn is_conditional(&self) -> bool {
		matches!(self, Self::String { text, quoted: false } if text.starts_with('[') && text.ends_with(']'))
	}
}

struct Parser<'a> {
	text: &'a str,
	pos: usize,
	options: ParseOptions,
}

impl<'a> Parser<'a> {
	fn object(&mut self, top_level: bool) -> Result<Vec<Node>, ParseError> {
		let mut children = Vec::new();
		loop {
			let key = match self.next()? {
				Token::End if top_level => return Ok(children),
				Token::End => return Err(self.fail("unexpected end of file inside a block")),
				Token::Close if !top_level => return Ok(children),
				Token::Close => return Err(self.fail("unexpected '}'")),
				Token::Open => return Err(self.fail("unexpected '{'")),
				token if token.is_conditional() => continue,
				Token::String { text, .. } => text,
			};
			let mut value = self.next()?;
			while value.is_conditional() {
				value = self.next()?;
			}
			let value = match value {
				Token::Open => Value::Object(self.object(false)?),
				Token::String { text, .. } => Value::String(text),
				Token::Close | Token::End => {
					return Err(self.fail(&format!("key \"{key}\" has no value")));
				}
			};
			children.push(Node { key, value });
		}
	}

	fn fail(&self, message: &str) -> ParseError {
		ParseError {
			line: 1 + self.text[..self.pos].matches('\n').count(),
			message: message.to_owned(),
		}
	}

	/// Borrows the text, not the parser, so `pos` can move while it is in use.
	fn rest(&self) -> &'a str {
		&self.text[self.pos..]
	}

	fn skip_space_and_comments(&mut self) {
		loop {
			let rest = self.rest();
			let trimmed = rest.trim_start();
			self.pos += rest.len() - trimmed.len();
			if !trimmed.starts_with("//") {
				return;
			}
			self.pos += trimmed.find('\n').map_or(trimmed.len(), |eol| eol + 1);
		}
	}

	fn next(&mut self) -> Result<Token, ParseError> {
		self.skip_space_and_comments();
		let Some(c) = self.rest().chars().next() else {
			return Ok(Token::End);
		};
		match c {
			'{' => {
				self.pos += 1;
				Ok(Token::Open)
			}
			'}' => {
				self.pos += 1;
				Ok(Token::Close)
			}
			'"' => self.quoted(),
			_ => {
				let rest = self.rest();
				let len = rest
					.find(|ch: char| ch.is_whitespace() || matches!(ch, '{' | '}' | '"'))
					.unwrap_or(rest.len());
				let text = rest[..len].to_owned();
				self.pos += len;
				Ok(Token::String {
					text,
					quoted: false,
				})
			}
		}
	}

	fn quoted(&mut self) -> Result<Token, ParseError> {
		self.pos += 1;
		let mut text = String::new();
		let mut chars = self.rest().char_indices();
		while let Some((i, c)) = chars.next() {
			match c {
				'"' => {
					self.pos += i + 1;
					return Ok(Token::String { text, quoted: true });
				}
				'\\' if self.options.escapes => match chars.next() {
					Some((_, 'n')) => text.push('\n'),
					Some((_, 't')) => text.push('\t'),
					Some((_, '\\')) => text.push('\\'),
					Some((_, '"')) => text.push('"'),
					Some((_, other)) => {
						text.push('\\');
						text.push(other);
					}
					None => text.push('\\'),
				},
				_ => text.push(c),
			}
		}
		self.pos = self.text.len();
		Err(self.fail("unterminated string"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parse(text: &str) -> Result<Node, ParseError> {
		super::parse(text, ParseOptions::default())
	}

	#[test]
	fn nested_objects() {
		let root = parse(
			"\"AppState\"\n{\n\t\"appid\"\t\t\"4000\"\n\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\"english\"\n\t}\n}\n",
		)
		.unwrap();
		let state = root.find("appstate").unwrap();
		assert!(state.is_object());
		assert_eq!(state.get("appid"), Some("4000"));
		assert_eq!(
			state.find("UserConfig").unwrap().get("language"),
			Some("english")
		);
		assert_eq!(state.get("missing"), None);
		assert_eq!(state.get("UserConfig"), None);
	}

	#[test]
	fn comments_and_unquoted_tokens() {
		let root = parse(
			"// header\nroot {\n  key value // trailing\n  \"a b\" \"c\" // Internal name\n}",
		)
		.unwrap();
		let node = root.find("root").unwrap();
		assert_eq!(node.get("key"), Some("value"));
		assert_eq!(node.get("a b"), Some("c"));
	}

	#[test]
	fn escapes() {
		let escaped = parse(r#""path" "C:\\Games\\Steam" "quote" "a\"b""#).unwrap();
		assert_eq!(escaped.get("path"), Some(r"C:\Games\Steam"));
		assert_eq!(escaped.get("quote"), Some("a\"b"));

		let raw = super::parse(r#""path" "models\props""#, ParseOptions::RAW).unwrap();
		assert_eq!(raw.get("path"), Some(r"models\props"));
	}

	#[test]
	fn conditionals_are_skipped() {
		let root = parse("\"a\" \"1\" [$WIN32]\n\"b\" [$LINUX] \"2\"\n").unwrap();
		assert_eq!(root.children().len(), 2);
		assert_eq!(root.get("a"), Some("1"));
		assert_eq!(root.get("b"), Some("2"));
	}

	#[test]
	fn bom_and_non_ascii() {
		let root = parse("\u{feff}\"name\" \"Гаррис мод\"").unwrap();
		assert_eq!(root.get("name"), Some("Гаррис мод"));
	}

	#[test]
	fn errors() {
		assert_eq!(parse("\"a\"\n{\n\"b\" \"c\"\n").unwrap_err().line, 4);
		assert!(parse("\"a\" \"unterminated").is_err());
		assert!(parse("}").is_err());
		assert!(parse("\"key\"").is_err());
		assert!(parse("{").is_err());
	}
}
