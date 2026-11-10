#include "core/Vdf.h"

#include <QTest>

class TestVdf : public QObject {
    Q_OBJECT

private slots:
    void nestedObjects()
    {
        const auto root = vdf::parse(u"\"AppState\"\n{\n\t\"appid\"\t\t\"4000\"\n\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\"english\"\n\t}\n}\n");
        QVERIFY(root);
        const vdf::Node *state = root->find(u"appstate");
        QVERIFY(state && state->isObject);
        QCOMPARE(state->get(u"appid"), "4000");
        QCOMPARE(state->find(u"UserConfig")->get(u"language"), "english");
        QCOMPARE(state->get(u"missing", "fallback"), "fallback");
    }

    void commentsAndUnquotedTokens()
    {
        const auto root = vdf::parse(u"// header\nroot {\n  key value // trailing\n  \"a b\" \"c\" // Internal name\n}");
        QVERIFY(root);
        const vdf::Node *node = root->find(u"root");
        QVERIFY(node);
        QCOMPARE(node->get(u"key"), "value");
        QCOMPARE(node->get(u"a b"), "c");
    }

    void escapes()
    {
        // No raw string literals here: moc mis-lexes them and drops the class.
        const auto escaped = vdf::parse(u"\"path\" \"C:\\\\Games\\\\Steam\" \"quote\" \"a\\\"b\"");
        QVERIFY(escaped);
        QCOMPARE(escaped->get(u"path"), "C:\\Games\\Steam");
        QCOMPARE(escaped->get(u"quote"), "a\"b");

        const auto raw = vdf::parse(u"\"path\" \"models\\props\"", nullptr, {.escapes = false});
        QVERIFY(raw);
        QCOMPARE(raw->get(u"path"), "models\\props");
    }

    void conditionalsAreSkipped()
    {
        const auto root = vdf::parse(u"\"a\" \"1\" [$WIN32]\n\"b\" [$LINUX] \"2\"\n");
        QVERIFY(root);
        QCOMPARE(root->children.size(), size_t(2));
        QCOMPARE(root->get(u"a"), "1");
        QCOMPARE(root->get(u"b"), "2");
    }

    void errors()
    {
        QString error;
        QVERIFY(!vdf::parse(u"\"a\"\n{\n\"b\" \"c\"\n", &error));
        QVERIFY(error.startsWith("line 4"));
        QVERIFY(!vdf::parse(u"\"a\" \"unterminated", &error));
        QVERIFY(!vdf::parse(u"}", &error));
        QVERIFY(!vdf::parse(u"\"key\"", &error));
    }
};

QTEST_APPLESS_MAIN(TestVdf)
#include "tst_vdf.moc"
