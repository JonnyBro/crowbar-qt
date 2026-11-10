#include "core/Vdf.h"

#include <QFile>

namespace vdf
{

	const Node *Node::find(QStringView name) const
	{
		for (const Node &child : children)
		{
			if (child.key.compare(name, Qt::CaseInsensitive) == 0)
				return &child;
		}
		return nullptr;
	}

	QString Node::get(QStringView name, const QString &fallback) const
	{
		const Node *child = find(name);
		return child && !child->isObject ? child->value : fallback;
	}

	namespace
	{

		enum class TokenType
		{
			String,
			Open,
			Close,
			End,
			Error
		};

		struct Token
		{
			TokenType type;
			QString text;
			bool quoted = false;
		};

		class Parser
		{
		public:
			Parser(QStringView text, ParseOptions options)
				: m_text(text), m_options(options)
			{
				if (m_text.startsWith(QChar(0xFEFF)))
					m_pos = 1;
			}

			bool parseObject(Node &parent, bool topLevel)
			{
				for (;;)
				{
					const Token key = next();
					switch (key.type)
					{
					case TokenType::End:
						return topLevel || fail("unexpected end of file inside a block");
					case TokenType::Close:
						return !topLevel || fail("unexpected '}'");
					case TokenType::Open:
						return fail("unexpected '{'");
					case TokenType::Error:
						return false;
					case TokenType::String:
						break;
					}
					if (isConditional(key))
						continue;

					Token value = next();
					while (isConditional(value))
						value = next();

					Node node;
					node.key = key.text;
					if (value.type == TokenType::Open)
					{
						node.isObject = true;
						if (!parseObject(node, false))
							return false;
					}
					else if (value.type == TokenType::String)
					{
						node.value = value.text;
					}
					else if (value.type == TokenType::Error)
					{
						return false;
					}
					else
					{
						return fail(QStringLiteral("key \"%1\" has no value").arg(key.text));
					}
					parent.children.push_back(std::move(node));
				}
			}

			QString error() const { return m_error; }

		private:
			// Platform conditionals such as [$WIN32] carry no data we need.
			static bool isConditional(const Token &token)
			{
				return token.type == TokenType::String && !token.quoted && token.text.startsWith(u'[') && token.text.endsWith(u']');
			}

			bool fail(const QString &message)
			{
				const int line = 1 + int(m_text.left(m_pos).count(u'\n'));
				m_error = QStringLiteral("line %1: %2").arg(line).arg(message);
				return false;
			}

			void skipSpaceAndComments()
			{
				while (m_pos < m_text.size())
				{
					const QChar c = m_text[m_pos];
					if (c.isSpace())
					{
						++m_pos;
					}
					else if (c == u'/' && m_pos + 1 < m_text.size() && m_text[m_pos + 1] == u'/')
					{
						const qsizetype eol = m_text.indexOf(u'\n', m_pos);
						m_pos = eol < 0 ? m_text.size() : eol + 1;
					}
					else
					{
						break;
					}
				}
			}

			Token next()
			{
				skipSpaceAndComments();
				if (m_pos >= m_text.size())
					return {TokenType::End, {}};

				const QChar c = m_text[m_pos];
				if (c == u'{')
				{
					++m_pos;
					return {TokenType::Open, {}};
				}
				if (c == u'}')
				{
					++m_pos;
					return {TokenType::Close, {}};
				}
				if (c == u'"')
					return quotedString();

				const qsizetype start = m_pos;
				while (m_pos < m_text.size())
				{
					const QChar ch = m_text[m_pos];
					if (ch.isSpace() || ch == u'{' || ch == u'}' || ch == u'"')
						break;
					++m_pos;
				}
				return {TokenType::String, m_text.mid(start, m_pos - start).toString()};
			}

			Token quotedString()
			{
				++m_pos;
				QString text;
				while (m_pos < m_text.size())
				{
					const QChar c = m_text[m_pos++];
					if (c == u'"')
						return {TokenType::String, text, true};
					if (c == u'\\' && m_options.escapes && m_pos < m_text.size())
					{
						const QChar escaped = m_text[m_pos++];
						switch (escaped.unicode())
						{
						case u'n':
							text += u'\n';
							break;
						case u't':
							text += u'\t';
							break;
						case u'\\':
							text += u'\\';
							break;
						case u'"':
							text += u'"';
							break;
						default:
							text += c;
							text += escaped;
							break;
						}
					}
					else
					{
						text += c;
					}
				}
				fail("unterminated string");
				return {TokenType::Error, {}};
			}

			QStringView m_text;
			ParseOptions m_options;
			qsizetype m_pos = 0;
			QString m_error;
		};

	} // namespace

	std::optional<Node> parse(QStringView text, QString *error, ParseOptions options)
	{
		Parser parser(text, options);
		Node root;
		root.isObject = true;
		if (!parser.parseObject(root, true))
		{
			if (error)
				*error = parser.error();
			return std::nullopt;
		}
		return root;
	}

	std::optional<Node> parseFile(const QString &path, QString *error, ParseOptions options)
	{
		QFile file(path);
		if (!file.open(QIODevice::ReadOnly))
		{
			if (error)
				*error = file.errorString();
			return std::nullopt;
		}
		return parse(QString::fromUtf8(file.readAll()), error, options);
	}

} // namespace vdf
