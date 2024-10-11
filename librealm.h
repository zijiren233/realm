#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#define TCP_TIMEOUT 5

#define TCP_KEEPALIVE 15

#define TCP_KEEPALIVE_PROBE 3

#define UDP_TIMEOUT 30

#define PROXY_PROTOCOL_VERSION 2

#define PROXY_PROTOCOL_TIMEOUT 5

typedef struct Features Features;



/**
 * 在C语言中使用Realm库的方法:
 *
 * 1. 包含头文件:
 *    #include "realm.h"
 *
 * 2. 调用start_realm函数:
 *    const char* listen_addr = start_realm("remote", "host", "path", true, false);
 *
 * 3. 关闭服务:
 *    stop_realm("remote", "host", "path", true, false);
 *
 * 注意:
 * - 确保已经正确编译并链接了Realm库
 * - start_realm函数不再阻塞，而是在后台运行
 */
const char *start_realm(const char *remote,
                        const char *host,
                        const char *path,
                        bool tls,
                        bool insecure);

void stop_realm(const char *remote, const char *host, const char *path, bool tls, bool insecure);
