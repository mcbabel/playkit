/* Playkit native mod API (C). A mod is a DLL in <Game>/Binaries/Win64/playkit-mods/ that exports
   int playkit_mod_init(const PkApi* api), called once on the game thread when the engine runs.
   Return 0 on success. */
#ifndef PLAYKIT_H
#define PLAYKIT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PLAYKIT_API_VERSION 1

/* pre hook: return non-zero to skip the original function. Post hook: return value ignored.
   params points to the function's parameters (and locals) laid out by their property offsets. */
typedef int (*PkHookFn)(void* user, void* object, void* function, void* params, void* result);
/* Return non-zero to stop iterating. */
typedef int (*PkObjectFn)(void* user, void* object);

typedef struct PkApi {
    uint32_t version;
    uint32_t size;
    void (*log)(const char* message);
    /* Full path, e.g. "/Script/Engine.Actor" or "/Game/Base/GI.GI_C:ReceiveInit" (. and : interchangeable). */
    void* (*find_object)(const char* path);
    /* Function of a class or one of its parents, by name. */
    void* (*find_function)(void* class_, const char* name);
    void* (*class_of)(void* object);
    int (*is_a)(void* object, void* class_);
    /* Copy the name/path into buffer; returns the full length. */
    int (*object_name)(void* object, char* buffer, int size);
    int (*object_path)(void* object, char* buffer, int size);
    /* class_ may be NULL for all objects. */
    void (*for_each_object)(void* class_, PkObjectFn fn, void* user);
    /* Byte offset of a property in a class, struct or function, -1 when missing. */
    int32_t (*property_offset)(void* struct_, const char* name);
    int32_t (*struct_size)(void* struct_);
    /* Hook a Blueprint (script) function. Returns a hook id, 0 on failure. */
    uint32_t (*hook)(void* function, PkHookFn pre, PkHookFn post, void* user);
    void (*unhook)(uint32_t id);
    /* UObject::ProcessEvent: call any UFunction with a parameter buffer. */
    void (*process_event)(void* object, void* function, void* params);
    /* Redirect native code at target to hook; returns a pointer that calls the original, or NULL. */
    void* (*detour)(void* target, void* hook);
    /* IDA style pattern search in the game executable, e.g. "48 8B ?? 05". */
    void* (*scan)(const char* pattern);
    void* (*image_base)(void);
    /* Hook a Blueprint function by path. Works before the class is loaded: the hook is attached
       as soon as the function exists. Returns a hook id, 0 on failure. */
    uint32_t (*hook_path)(const char* path, PkHookFn pre, PkHookFn post, void* user);
} PkApi;

#ifdef _WIN32
#define PLAYKIT_EXPORT __declspec(dllexport)
#else
#define PLAYKIT_EXPORT
#endif

#ifdef __cplusplus
}
#endif

#endif
