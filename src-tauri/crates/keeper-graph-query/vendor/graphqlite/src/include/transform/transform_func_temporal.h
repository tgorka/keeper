/*
 * transform_func_temporal.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_temporal.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_TEMPORAL_H
#define TRANSFORM_FUNC_TEMPORAL_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_timestamp_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_date_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_time_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_datetime_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_duration_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_datetime_from_epoch_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_date_truncate_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_datetime_truncate_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_localdatetime_truncate_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_time_truncate_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_localtime_truncate_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_duration_between_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_duration_in_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_date_add_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_TEMPORAL_H */
