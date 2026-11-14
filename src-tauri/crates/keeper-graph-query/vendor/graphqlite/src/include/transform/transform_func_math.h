/*
 * transform_func_math.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_math.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_MATH_H
#define TRANSFORM_FUNC_MATH_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_math_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_round_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_noarg_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_atan2_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_MATH_H */
