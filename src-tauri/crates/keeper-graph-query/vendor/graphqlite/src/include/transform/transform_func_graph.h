/*
 * transform_func_graph.h
 *    Declarations for functions implemented in src/backend/transform/transform_func_graph.c.
 *    Split out from transform_functions.h (I-0041 C5).
 */

#ifndef TRANSFORM_FUNC_GRAPH_H
#define TRANSFORM_FUNC_GRAPH_H

#include "transform/cypher_transform.h"
#include "parser/cypher_ast.h"

int transform_pagerank_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_top_pagerank_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_personalized_pagerank_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_label_propagation_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_community_of_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_community_members_function(cypher_transform_context *ctx, cypher_function_call *func);
int transform_community_count_function(cypher_transform_context *ctx, cypher_function_call *func);

#endif /* TRANSFORM_FUNC_GRAPH_H */
