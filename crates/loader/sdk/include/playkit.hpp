// Playkit native mod API, C++ layer. Include this, or the generated playkit_sdk.hpp.
#pragma once
#include "playkit.h"

#include <cstdarg>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <functional>
#include <string>
#include <vector>

namespace pk {

inline const PkApi* api = nullptr;

namespace types {

struct UObjectBase {
    void** VTable;
    int32_t ObjectFlags;
    int32_t InternalIndex;
    struct UClass* ClassPrivate;
    uint32_t NameIndex;
    uint32_t NameNumber;
    struct UObject* OuterPrivate;
};
static_assert(sizeof(UObjectBase) == 0x28, "UObjectBase size");

struct FName {
    uint32_t Index = 0;
    uint32_t Number = 0;
};

template <typename T>
struct TArray {
    T* Data = nullptr;
    int32_t Num = 0;
    int32_t Max = 0;
    T* begin() const { return Data; }
    T* end() const { return Data + Num; }
    T& operator[](int32_t i) const { return Data[i]; }
    int32_t size() const { return Num; }
};

struct FString {
    TArray<wchar_t> Chars;
    std::string str() const {
        std::string s;
        for (int32_t i = 0; i + 1 < Chars.Num; ++i) s.push_back(Chars.Data[i] < 128 ? char(Chars.Data[i]) : '?');
        return s;
    }
};

struct FText {
    uint8_t Data[24];
};
struct FScriptDelegate {
    uint8_t Data[16];
};
struct FMulticastScriptDelegate {
    uint8_t Data[16];
};
struct FScriptInterface {
    void* Object;
    void* Interface;
};
template <typename T>
struct TWeakObjectPtr {
    int32_t Index;
    int32_t Serial;
};
template <typename T>
struct TSoftObjectPtr {
    uint8_t Data[40];
};
struct TSoftClassPtr {
    uint8_t Data[40];
};
struct TSetOpaque {
    uint8_t Data[80];
};
struct TMapOpaque {
    uint8_t Data[80];
};

struct UObject : UObjectBase {};
struct UClass : UObject {};
struct UFunction : UObject {};

}  // namespace types

using namespace types;

inline void log(const char* fmt, ...) {
    char buf[1024];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap);
    api->log(buf);
}

template <typename T = UObject>
T* find(const char* path) {
    return static_cast<T*>(api->find_object(path));
}

inline UFunction* function(void* class_, const char* name) {
    return static_cast<UFunction*>(api->find_function(class_, name));
}

inline void process_event(void* object, void* function, void* params) { api->process_event(object, function, params); }

inline std::string name(void* object) {
    char buf[256];
    int n = api->object_name(object, buf, sizeof buf);
    return n < (int)sizeof buf ? std::string(buf) : std::string(buf, sizeof buf - 1);
}

inline std::string path(void* object) {
    std::vector<char> buf(1024);
    int n = api->object_path(object, buf.data(), (int)buf.size());
    if (n >= (int)buf.size()) {
        buf.resize(n + 1);
        api->object_path(object, buf.data(), (int)buf.size());
    }
    return std::string(buf.data());
}

inline bool is_a(void* object, void* class_) { return api->is_a(object, class_) != 0; }

/// Default object of a class, e.g. for calling static Blueprint library functions.
inline UObject* default_object(void* class_) {
    std::string p = path(class_);
    size_t dot = p.find_last_of(".:");
    std::string cdo = p.substr(0, dot + 1) + "Default__" + p.substr(dot + 1);
    return find<UObject>(cdo.c_str());
}

template <typename T = UObject>
std::vector<T*> objects_of(UClass* class_) {
    std::vector<T*> out;
    api->for_each_object(
        class_,
        [](void* user, void* o) {
            static_cast<std::vector<T*>*>(user)->push_back(static_cast<T*>(o));
            return 0;
        },
        &out);
    return out;
}

/// Typed access to a property by name, resolved once per call site.
template <typename T>
T& field(void* object, void* struct_, const char* name) {
    int32_t off = api->property_offset(struct_, name);
    return *reinterpret_cast<T*>(static_cast<uint8_t*>(object) + (off < 0 ? 0 : off));
}

struct Call {
    UObject* object;
    UFunction* function;
    uint8_t* params;
    void* result;
    bool skip = false;
    /// Parameter or local of the hooked function, by name.
    template <typename T>
    T& param(const char* name) const {
        return *reinterpret_cast<T*>(params + api->property_offset(function, name));
    }
};

using Handler = std::function<void(Call&)>;

namespace detail {
struct HookData {
    Handler before, after;
};
inline int run(void* user, void* object, void* function, void* params, void* result, bool pre) {
    auto* d = static_cast<HookData*>(user);
    Call c{static_cast<UObject*>(object), static_cast<UFunction*>(function), static_cast<uint8_t*>(params), result};
    auto& h = pre ? d->before : d->after;
    if (h) h(c);
    return c.skip ? 1 : 0;
}
inline int pre(void* u, void* o, void* f, void* p, void* r) { return run(u, o, f, p, r, true); }
inline int post(void* u, void* o, void* f, void* p, void* r) { return run(u, o, f, p, r, false); }
}  // namespace detail

/// Hook a Blueprint function by path ("/Game/X.X_C:Func"). Set call.skip in `before` to skip it.
/// Works before the Blueprint is loaded: the hook is attached as soon as the function exists.
inline uint32_t hook(const char* function_path, Handler before, Handler after = nullptr) {
    auto* d = new detail::HookData{std::move(before), std::move(after)};
    return api->hook_path(function_path, d->before ? detail::pre : nullptr, d->after ? detail::post : nullptr, d);
}

}  // namespace pk

/// Usage:  PLAYKIT_MOD() { pk::log("hello"); pk::hook(...); }
#define PLAYKIT_MOD()                                                     \
    static void playkit_main();                                           \
    extern "C" PLAYKIT_EXPORT int playkit_mod_init(const PkApi* api_) {   \
        if (!api_ || api_->version < PLAYKIT_API_VERSION) return 1;       \
        pk::api = api_;                                                   \
        playkit_main();                                                   \
        return 0;                                                         \
    }                                                                     \
    static void playkit_main()
