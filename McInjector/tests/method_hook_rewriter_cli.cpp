// method_hook_rewriter_cli.cpp
// Command line wrapper around mhook::Rewrite for the Java verification tests and for
// checking the real hook table against real Minecraft classes.
//
//   method_hook_rewriter_cli <in.class> <out.class> <helper/internal/Name> <spec>...
//   spec = name1|name2:shape:guardName[:valueName[:forbidFlagsHex]]
//
// Exit codes: 0 rewritten or already hooked, 3 nothing matched, 1 failure.
#include "../src/main/cpp/method_hook_rewriter.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>
#include <sstream>

static std::vector<std::string> Split(const std::string& s, char sep) {
    std::vector<std::string> out;
    std::string cur;
    for (size_t i = 0; i < s.size(); ++i) {
        if (s[i] == sep) { out.push_back(cur); cur.clear(); } else cur += s[i];
    }
    out.push_back(cur);
    return out;
}

int main(int argc, char** argv) {
    if (argc < 5) {
        std::fprintf(stderr, "usage: %s in.class out.class helper spec...\n", argv[0]);
        return 1;
    }
    std::ifstream in(argv[1], std::ios::binary);
    if (!in) { std::fprintf(stderr, "cannot read %s\n", argv[1]); return 1; }
    std::vector<unsigned char> data((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());

    std::vector<std::vector<std::string> > nameStore;
    std::vector<std::vector<const char*> > namePtrs;
    std::vector<std::string> shapes, guards, values;
    std::vector<unsigned> forbids;
    std::vector<mhook::HookSpec> specs;
    for (int i = 4; i < argc; ++i) {
        std::vector<std::string> parts = Split(argv[i], ':');
        if (parts.size() < 3) { std::fprintf(stderr, "bad spec %s\n", argv[i]); return 1; }
        nameStore.push_back(Split(parts[0], '|'));
        shapes.push_back(parts[1]);
        guards.push_back(parts[2]);
        values.push_back(parts.size() > 3 ? parts[3] : std::string());
        forbids.push_back(parts.size() > 4 ? (unsigned)std::strtoul(parts[4].c_str(), nullptr, 16) : 0u);
    }
    namePtrs.resize(nameStore.size());
    for (size_t i = 0; i < nameStore.size(); ++i) {
        for (size_t j = 0; j < nameStore[i].size(); ++j) namePtrs[i].push_back(nameStore[i][j].c_str());
        namePtrs[i].push_back(nullptr);
        mhook::HookSpec s;
        s.methodNames = namePtrs[i].data();
        s.shape = shapes[i].c_str();
        s.guardName = guards[i].c_str();
        s.valueName = values[i].empty() ? nullptr : values[i].c_str();
        s.forbidFlags = forbids[i];
        specs.push_back(s);
    }

    mhook::RewriteResult r = mhook::Rewrite(data.data(), data.size(), specs.data(), specs.size(), argv[3]);
    if (!r.ok) { std::fprintf(stderr, "rewrite failed: %s\n", r.error.c_str()); return 1; }
    if (!r.changed && r.alreadyHooked == 0) { std::printf("no match\n"); return 3; }
    const std::vector<unsigned char>& bytes = r.changed ? r.bytes : data;
    std::ofstream out(argv[2], std::ios::binary);
    out.write((const char*)bytes.data(), (std::streamsize)bytes.size());
    std::printf("hooked=%d already=%d bytes=%zu\n", r.hooked, r.alreadyHooked, bytes.size());
    return 0;
}
