/*
 * transform_func_aggregate.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_aggregate.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_AGGREGATE_H
#define TRANSFORM_FUNC_AGGREGATE_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_count_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_aggregate_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_aggregate_with_property(cypher_transform_context *ctx, cypher_function_call *func, cypher_property *prop);
int transform_type_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_stdev_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_percentile_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_AGGREGATE_H */
