/*
 * transform_func_entity.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_entity.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_ENTITY_H
#define TRANSFORM_FUNC_ENTITY_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_id_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_labels_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_properties_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_keys_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_graph_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_ENTITY_H */
