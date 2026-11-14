/*
 * transform_func_geo.c
 *    Geospatial point construction and queries. Moved from
 *    transform_func_list.c (I-0040 M9).
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "transform/cypher_transform.h"
#include "transform/transform_func_geo.h"
#include "parser/cypher_ast.h"
#include "parser/cypher_debug.h"

/* Transform point() function
 * point({x, y}) — 2D Cartesian point (SRID 7203)
 * point({x, y, z}) — 3D Cartesian point
 * point({latitude, longitude}) — 2D geographic WGS-84 point (SRID 4326)
 * point({latitude, longitude, height}) — 3D geographic point
 *
 * Stored as JSON object with type metadata for distance calculations.
 */
int transform_point_function(cypher_transform_context *ctx, cypher_function_call *func_call)
{
    CYPHER_DEBUG("Transforming point() function");

    if (!func_call->args || func_call->args->count != 1) {
        ctx->has_error = true;
        ctx->error_message = strdup("point() requires exactly one map argument");
        return -1;
    }

    ast_node *arg = func_call->args->items[0];

    /* Build a JSON object that preserves all input keys plus a _srid for distance calculations.
     * The point function detects Cartesian (x,y) vs Geographic (latitude,longitude) by key names.
     * We use json_object to normalize the representation. */
    append_sql(ctx, "(SELECT CASE "
               "WHEN json_extract(json(");
    if (transform_expression(ctx, arg) < 0) return -1;
    append_sql(ctx, "), '$.latitude') IS NOT NULL THEN json_object("
               "'srid', 4326, "
               "'latitude', json_extract(json(");
    if (transform_expression(ctx, arg) < 0) return -1;
    append_sql(ctx, "), '$.latitude'), "
               "'longitude', json_extract(json(");
    if (transform_expression(ctx, arg) < 0) return -1;
    append_sql(ctx, "), '$.longitude'), "
               "'height', json_extract(json(");
    if (transform_expression(ctx, arg) < 0) return -1;
    append_sql(ctx, "), '$.height')) "
               "ELSE json_object("
               "'srid', 7203, "
               "'x', json_extract(json(");
    if (transform_expression(ctx, arg) < 0) return -1;
    append_sql(ctx, "), '$.x'), "
               "'y', json_extract(json(");
    if (transform_expression(ctx, arg) < 0) return -1;
    append_sql(ctx, "), '$.y'), "
               "'z', json_extract(json(");
    if (transform_expression(ctx, arg) < 0) return -1;
    append_sql(ctx, "), '$.z')) END)");

    return 0;
}

/* Transform point.distance(p1, p2) / distance(p1, p2)
 * For Cartesian points (srid 7203): sqrt((x2-x1)^2 + (y2-y1)^2)
 * For Geographic points (srid 4326): haversine formula in meters
 */
int transform_point_distance_function(cypher_transform_context *ctx, cypher_function_call *func_call)
{
    CYPHER_DEBUG("Transforming point.distance() function");

    if (!func_call->args || func_call->args->count != 2) {
        ctx->has_error = true;
        ctx->error_message = strdup("point.distance() requires exactly two point arguments");
        return -1;
    }

    /* Detect SRID from first point and use appropriate formula */
    append_sql(ctx, "(SELECT CASE WHEN json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.srid') = 4326 THEN "
               /* Haversine formula: 2 * R * asin(sqrt(hav(dlat) + cos(lat1)*cos(lat2)*hav(dlon))) */
               "6371000.0 * 2.0 * ASIN(SQRT("
               "((1.0 - COS((json_extract(");
    if (transform_expression(ctx, func_call->args->items[1]) < 0) return -1;
    append_sql(ctx, ", '$.latitude') - json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.latitude')) * 3.141592653589793 / 180.0)) / 2.0) + "
               "COS(json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.latitude') * 3.141592653589793 / 180.0) * "
               "COS(json_extract(");
    if (transform_expression(ctx, func_call->args->items[1]) < 0) return -1;
    append_sql(ctx, ", '$.latitude') * 3.141592653589793 / 180.0) * "
               "((1.0 - COS((json_extract(");
    if (transform_expression(ctx, func_call->args->items[1]) < 0) return -1;
    append_sql(ctx, ", '$.longitude') - json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.longitude')) * 3.141592653589793 / 180.0)) / 2.0)"
               ")) "
               "ELSE "
               /* Euclidean distance for Cartesian */
               "SQRT("
               "POWER(json_extract(");
    if (transform_expression(ctx, func_call->args->items[1]) < 0) return -1;
    append_sql(ctx, ", '$.x') - json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.x'), 2) + "
               "POWER(json_extract(");
    if (transform_expression(ctx, func_call->args->items[1]) < 0) return -1;
    append_sql(ctx, ", '$.y') - json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.y'), 2)"
               ") END)");

    return 0;
}

/* Transform point.withinBBox(point, lowerLeft, upperRight)
 * Returns true if point is within the bounding box defined by two corner points.
 */
int transform_point_within_bbox_function(cypher_transform_context *ctx, cypher_function_call *func_call)
{
    CYPHER_DEBUG("Transforming point.withinBBox() function");

    if (!func_call->args || func_call->args->count != 3) {
        ctx->has_error = true;
        ctx->error_message = strdup("point.withinBBox() requires three arguments: point, lowerLeft, upperRight");
        return -1;
    }

    /* Check if geographic or Cartesian */
    append_sql(ctx, "(SELECT CASE WHEN json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.srid') = 4326 THEN ("
               "json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.latitude') >= json_extract(");
    if (transform_expression(ctx, func_call->args->items[1]) < 0) return -1;
    append_sql(ctx, ", '$.latitude') AND json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.latitude') <= json_extract(");
    if (transform_expression(ctx, func_call->args->items[2]) < 0) return -1;
    append_sql(ctx, ", '$.latitude') AND json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.longitude') >= json_extract(");
    if (transform_expression(ctx, func_call->args->items[1]) < 0) return -1;
    append_sql(ctx, ", '$.longitude') AND json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.longitude') <= json_extract(");
    if (transform_expression(ctx, func_call->args->items[2]) < 0) return -1;
    append_sql(ctx, ", '$.longitude'))"
               " ELSE ("
               "json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.x') >= json_extract(");
    if (transform_expression(ctx, func_call->args->items[1]) < 0) return -1;
    append_sql(ctx, ", '$.x') AND json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.x') <= json_extract(");
    if (transform_expression(ctx, func_call->args->items[2]) < 0) return -1;
    append_sql(ctx, ", '$.x') AND json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.y') >= json_extract(");
    if (transform_expression(ctx, func_call->args->items[1]) < 0) return -1;
    append_sql(ctx, ", '$.y') AND json_extract(");
    if (transform_expression(ctx, func_call->args->items[0]) < 0) return -1;
    append_sql(ctx, ", '$.y') <= json_extract(");
    if (transform_expression(ctx, func_call->args->items[2]) < 0) return -1;
    append_sql(ctx, ", '$.y'))"
               " END)");

    return 0;
}

