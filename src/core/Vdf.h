#pragma once

#include <QString>

#include <optional>
#include <vector>

namespace vdf {

struct Node {
    QString key;
    QString value;
    std::vector<Node> children;
    bool isObject = false;

    const Node *find(QStringView name) const;
    QString get(QStringView name, const QString &fallback = {}) const;
};

struct ParseOptions {
    // Steam's own files escape backslashes; game files such as gameinfo.txt do not.
    bool escapes = true;
};

// The returned node is a synthetic root whose children are the top-level pairs.
std::optional<Node> parse(QStringView text, QString *error = nullptr, ParseOptions options = {});
std::optional<Node> parseFile(const QString &path, QString *error = nullptr, ParseOptions options = {});

} // namespace vdf
