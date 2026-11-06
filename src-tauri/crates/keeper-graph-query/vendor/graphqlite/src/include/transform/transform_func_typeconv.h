/*
 * transform_func_typeconv.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_typeconv.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_TYPECONV_H
#define TRANSFORM_FUNC_TYPECONV_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_tostring_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_type_conversion_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_type_conversion_ornull_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_nullif_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_valuetype_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_TYPECONV_H */
