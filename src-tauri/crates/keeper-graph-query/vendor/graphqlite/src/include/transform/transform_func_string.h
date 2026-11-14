/*
 * transform_func_string.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_string.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_STRING_H
#define TRANSFORM_FUNC_STRING_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_string_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_substring_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_replace_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_split_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_leftright_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_pattern_match_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_STRING_H */
