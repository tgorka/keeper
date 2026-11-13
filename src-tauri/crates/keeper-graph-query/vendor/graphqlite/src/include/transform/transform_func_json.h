/*
 * transform_func_json.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_json.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_JSON_H
#define TRANSFORM_FUNC_JSON_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_json_get_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_json_keys_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_json_type_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_isempty_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_JSON_H */
