/* First-party declarations only. Allocation bodies are the audited Rust U03-U08. */
#ifndef KEEPER_GRAPH_ALLOCATION_H
#define KEEPER_GRAPH_ALLOCATION_H
#include <stdlib.h>
#include <string.h>
void *keeper_graph_malloc(size_t);
void keeper_graph_free(void *);
void *keeper_graph_calloc(size_t, size_t);
void *keeper_graph_realloc(void *, size_t);
char *keeper_graph_strdup(const char *);
char *keeper_graph_strndup(const char *, size_t);
#define malloc keeper_graph_malloc
#define free keeper_graph_free
#define calloc keeper_graph_calloc
#define realloc keeper_graph_realloc
#define strdup keeper_graph_strdup
#define strndup keeper_graph_strndup
#endif
