#include <iostream>
#include <string>
#include <vector>

#include "../src/main/cpp/bounded_newline_buffer.h"

static int g_failures = 0;

static void ExpectTrue(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void ExpectEq(
    const std::string& expected,
    const std::string& actual,
    const char* message)
{
    if (expected != actual) {
        std::cerr << "FAIL: " << message
                  << " expected='" << expected
                  << "' actual='" << actual << "'" << std::endl;
        ++g_failures;
    }
}

static void TestSplitAndMultipleLines()
{
    lc::BoundedNewlineBuffer buffer(32);
    std::vector<std::string> lines;

    ExpectTrue(buffer.Append("fir", 3, &lines) == 0, "split prefix discard count");
    ExpectTrue(lines.empty(), "split prefix should not complete a line");
    ExpectTrue(buffer.Append("st\r\nsecond\n", 11, &lines) == 0, "completed lines discard count");
    ExpectTrue(lines.size() == 2, "two lines should complete");
    if (lines.size() == 2) {
        ExpectEq("first", lines[0], "CRLF line");
        ExpectEq("second", lines[1], "LF line");
    }
}

static void TestOversizedLineIsDiscardedAndRecoveryContinues()
{
    lc::BoundedNewlineBuffer buffer(5);
    std::vector<std::string> lines;
    const std::string input = "123456789\nvalid\n";

    std::size_t discarded = buffer.Append(input.data(), input.size(), &lines);

    ExpectTrue(discarded == 1, "one oversized line should be reported");
    ExpectTrue(lines.size() == 1, "valid line after oversized input should survive");
    if (lines.size() == 1)
        ExpectEq("valid", lines[0], "line after oversized input");
    ExpectTrue(buffer.BufferedBytes() == 0, "buffer should be empty after complete lines");
    ExpectTrue(!buffer.IsDiscardingOversizedLine(), "discard mode should end at newline");
}

static void TestPartialOversizedLineStaysBounded()
{
    lc::BoundedNewlineBuffer buffer(5);
    std::vector<std::string> lines;

    std::size_t discarded = buffer.Append("123456789", 9, &lines);

    ExpectTrue(discarded == 0, "unterminated oversized line is not complete yet");
    ExpectTrue(lines.empty(), "unterminated oversized line should not publish");
    ExpectTrue(buffer.BufferedBytes() == 0, "oversized partial data should be released");
    ExpectTrue(buffer.IsDiscardingOversizedLine(), "reader should discard until newline");
}

static void TestLineAtLimitAcceptsCrLf()
{
    lc::BoundedNewlineBuffer buffer(5);
    std::vector<std::string> lines;

    std::size_t discarded = buffer.Append("12345\r\n", 7, &lines);

    ExpectTrue(discarded == 0, "CRLF should not count against the line limit");
    ExpectTrue(lines.size() == 1, "line at limit should publish");
    if (lines.size() == 1)
        ExpectEq("12345", lines[0], "line at limit");
}

int main()
{
    TestSplitAndMultipleLines();
    TestOversizedLineIsDiscardedAndRecoveryContinues();
    TestPartialOversizedLineStaysBounded();
    TestLineAtLimitAcceptsCrLf();

    if (g_failures != 0) {
        std::cerr << "bounded_newline_buffer_tests failed: " << g_failures << std::endl;
        return 1;
    }

    std::cout << "bounded_newline_buffer_tests: all passed" << std::endl;
    return 0;
}
