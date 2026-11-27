// visuals_hooks_check.cpp
// Applies the real hook table to real game classes and reports every mismatch.
//
//   visuals_hooks_check <legacy|modern> <classesRoot> <outRoot>
//
// classesRoot holds the extracted jar (net/minecraft/... .class). Each rewritten class is
// written under outRoot for the JVM verifier step (HookVerifyTest). Exit code 0 only when
// every hook class exists, every spec matched exactly one method, and no rewrite failed.
#include "../src/main/cpp/visuals_hooks.h"

#include <cstdio>
#include <fstream>
#include <iterator>
#include <string>
#include <sys/stat.h>

static bool MakeDirs(const std::string& path) {
    std::string cur;
    for (size_t i = 0; i < path.size(); ++i) {
        cur += path[i];
        if (path[i] == '/' && cur.size() > 1) mkdir(cur.c_str(), 0755);
    }
    return mkdir(path.c_str(), 0755) == 0 || true;
}

int main(int argc, char** argv) {
    if (argc != 4) { std::fprintf(stderr, "usage: %s legacy|modern classesRoot outRoot\n", argv[0]); return 2; }
    const visuals::Generation gen = std::string(argv[1]) == "legacy" ? visuals::Generation::Legacy189 : visuals::Generation::Modern26;
    size_t n = 0;
    const visuals::HookClass* classes = visuals::HookClasses(gen, &n);
    int bad = 0;
    for (size_t i = 0; i < n; ++i) {
        const std::string rel = std::string(classes[i].internalName) + ".class";
        std::ifstream in(std::string(argv[2]) + "/" + rel, std::ios::binary);
        if (!in) { std::printf("MISSING %s\n", rel.c_str()); ++bad; continue; }
        std::vector<unsigned char> data((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
        mhook::RewriteResult r = mhook::Rewrite(data.data(), data.size(), classes[i].specs, classes[i].count, visuals::kHelperInternalName);
        if (!r.ok) { std::printf("FAIL    %s: %s\n", rel.c_str(), r.error.c_str()); ++bad; continue; }
        if (r.hooked != (int)classes[i].count) {
            std::printf("COUNT   %s: %d methods hooked, %zu specs\n", rel.c_str(), r.hooked, classes[i].count);
            ++bad;
            continue;
        }
        const std::string out = std::string(argv[3]) + "/" + rel;
        MakeDirs(out.substr(0, out.rfind('/')));
        std::ofstream o(out, std::ios::binary);
        o.write((const char*)r.bytes.data(), (std::streamsize)r.bytes.size());
        std::printf("OK      %s: %d hooks, %zu -> %zu bytes\n", rel.c_str(), r.hooked, data.size(), r.bytes.size());
        for (size_t k = 0; k < classes[i].count; ++k) {
            std::string guard, value;
            visuals::ExpectedHelperDescriptors(classes[i].specs[k], &guard, &value);
            std::printf("HOOKMETHOD %s %s\n", classes[i].specs[k].guardName, guard.c_str());
            if (classes[i].specs[k].valueName) std::printf("HOOKMETHOD %s %s\n", classes[i].specs[k].valueName, value.c_str());
        }
    }
    return bad == 0 ? 0 : 1;
}
