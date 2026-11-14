/*
 * transform_func_list.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_list.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_LIST_H
#define TRANSFORM_FUNC_LIST_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_coalesce_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_list_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_range_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_collect_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_randomuuid_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_length_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_LIST_H */
