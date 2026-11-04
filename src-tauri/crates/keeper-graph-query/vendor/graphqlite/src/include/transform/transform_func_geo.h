/*
 * transform_func_geo.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_geo.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_GEO_H
#define TRANSFORM_FUNC_GEO_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_point_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_point_distance_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_point_within_bbox_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_GEO_H */
