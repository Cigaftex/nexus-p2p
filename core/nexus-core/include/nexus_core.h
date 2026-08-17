#ifndef NEXUS_CORE_H
#define NEXUS_CORE_H

#ifdef __cplusplus
extern "C" {
#endif
typedef struct FfiHandle FfiHandle;
FfiHandle *nexus_create(const char *config_json);
char *nexus_call(FfiHandle *handle, const char *command_json);
char *nexus_last_error(void);
void nexus_string_free(char *value);
void nexus_destroy(FfiHandle *handle);

#ifdef __cplusplus
}
#endif
#endif
