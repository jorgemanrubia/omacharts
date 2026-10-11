/*
 * Preloaded into omacharts by AppRun: hands every program omacharts starts the
 * environment the AppImage was started with, not the one AppRun set up for the
 * bundled GTK. The browser opened for the thinkorswim sign-in, xdg-open, ps and
 * kill are host programs, and a host program that picks up our pixbuf loaders
 * or GIO settings loads libraries linked against the bundled glibc into itself.
 *
 * AppRun lists the variables it touched in APPIMAGE_RESTORE_VARS and keeps each
 * one's original value in APPIMAGE_ORIG_<NAME> (absent if it was unset). This
 * applies that list to the environment of each exec and posix_spawn, then drops
 * the bookkeeping variables and itself (LD_PRELOAD) from it.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <spawn.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern char **environ;

static int starts(const char *s, const char *p) { return strncmp(s, p, strlen(p)) == 0; }

/* A copy of envp with AppRun's changes undone. Leaked on purpose: the exec
 * replaces the image, and on failure a few strings are cheaper than a bug. */
static char **restored(char *const envp[])
{
	size_t n = 0;
	while (envp && envp[n])
		n++;
	const char *list = NULL;
	for (size_t i = 0; i < n; i++)
		if (starts(envp[i], "APPIMAGE_RESTORE_VARS="))
			list = envp[i] + strlen("APPIMAGE_RESTORE_VARS=");
	if (!list)
		return (char **)envp;

	char **out = calloc(n + 64, sizeof(char *));
	size_t m = 0;
	for (size_t i = 0; i < n; i++) {
		const char *e = envp[i];
		if (starts(e, "APPIMAGE_RESTORE_VARS=") || starts(e, "APPIMAGE_ORIG_") || starts(e, "LD_PRELOAD="))
			continue;
		int touched = 0;
		for (const char *p = list; *p;) {
			const char *q = strchr(p, ' ');
			size_t len = q ? (size_t)(q - p) : strlen(p);
			if (strncmp(e, p, len) == 0 && e[len] == '=') {
				touched = 1;
				break;
			}
			p += len + (q ? 1 : 0);
		}
		if (!touched && m < n + 63)
			out[m++] = (char *)e;
	}
	/* Put back what was there before AppRun ran. */
	for (const char *p = list; *p;) {
		const char *q = strchr(p, ' ');
		size_t len = q ? (size_t)(q - p) : strlen(p);
		char key[256];
		if (len + 15 < sizeof key) {
			memcpy(key, "APPIMAGE_ORIG_", 14);
			memcpy(key + 14, p, len);
			key[14 + len] = '=';
			key[15 + len] = '\0';
			for (size_t i = 0; i < n; i++)
				if (starts(envp[i], key) && m < n + 63) {
					char *s = malloc(len + 1 + strlen(envp[i] + 15 + len) + 1);
					if (s) {
						memcpy(s, p, len);
						s[len] = '=';
						strcpy(s + len + 1, envp[i] + 15 + len);
						out[m++] = s;
					}
				}
		}
		p += len + (q ? 1 : 0);
	}
	out[m] = NULL;
	return out;
}

#define REAL(name) ((typeof(&name))dlsym(RTLD_NEXT, #name))

int execve(const char *path, char *const argv[], char *const envp[])
{
	return REAL(execve)(path, argv, restored(envp));
}
int execvpe(const char *file, char *const argv[], char *const envp[])
{
	return REAL(execvpe)(file, argv, restored(envp));
}
int execv(const char *path, char *const argv[])
{
	return REAL(execve)(path, argv, restored(environ));
}
int execvp(const char *file, char *const argv[])
{
	return REAL(execvpe)(file, argv, restored(environ));
}
int posix_spawn(pid_t *pid, const char *path, const posix_spawn_file_actions_t *fa, const posix_spawnattr_t *at,
		char *const argv[], char *const envp[])
{
	return REAL(posix_spawn)(pid, path, fa, at, argv, restored(envp));
}
int posix_spawnp(pid_t *pid, const char *file, const posix_spawn_file_actions_t *fa, const posix_spawnattr_t *at,
		 char *const argv[], char *const envp[])
{
	return REAL(posix_spawnp)(pid, file, fa, at, argv, restored(envp));
}
