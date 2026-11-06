/*
 * runtime/udf_register.h
 *    Registration entry point for the cache-less helper UDFs.
 *    Replaces the historical `extern int graphqlite_register_helper_udfs(sqlite3 *db);`
 *    forward-decls scattered across transform/executor sources.
 */

#ifndef RUNTIME_UDF_REGISTER_H
#define RUNTIME_UDF_REGISTER_H

#include "graphqlite_sqlite.h"

int graphqlite_register_helper_udfs(sqlite3 *db);

#endif /* RUNTIME_UDF_REGISTER_H */
