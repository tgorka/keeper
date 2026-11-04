/* A Bison parser, made by GNU Bison 3.8.2.  */

/* Skeleton implementation for Bison GLR parsers in C

   Copyright (C) 2002-2015, 2018-2021 Free Software Foundation, Inc.

   This program is free software: you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation, either version 3 of the License, or
   (at your option) any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program.  If not, see <https://www.gnu.org/licenses/>.  */

/* As a special exception, you may create a larger work that contains
   part or all of the Bison parser skeleton and distribute that work
   under terms of your choice, so long as that work isn't itself a
   parser generator using the skeleton or a modified version thereof
   as a parser skeleton.  Alternatively, if you modify or redistribute
   the parser skeleton itself, you may (at your option) remove this
   special exception, which will cause the skeleton and the resulting
   Bison output files to be licensed under the GNU General Public
   License without this special exception.

   This special exception was added by the Free Software Foundation in
   version 2.2 of Bison.  */

/* C GLR parser skeleton written by Paul Hilfinger.  */

/* DO NOT RELY ON FEATURES THAT ARE NOT DOCUMENTED in the manual,
   especially those whose name start with YY_ or yy_.  They are
   private implementation details that can be changed or removed.  */

/* Identify Bison output, and Bison version.  */
#define YYBISON 30802

/* Bison version string.  */
#define YYBISON_VERSION "3.8.2"

/* Skeleton name.  */
#define YYSKELETON_NAME "glr.c"

/* Pure parsers.  */
#define YYPURE 1



/* Substitute the type names.  */
#define YYSTYPE CYPHER_YYSTYPE
#define YYLTYPE CYPHER_YYLTYPE
/* Substitute the variable and function names.  */
#define yyparse cypher_yyparse
#define yylex   cypher_yylex
#define yyerror cypher_yyerror
#define yydebug cypher_yydebug

/* First part of user prologue.  */
#line 1 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"

/*
 * Cypher Grammar for GraphQLite
 * Simplified version based on Apache AGE grammar for SQLite compatibility
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

#include "parser/cypher_ast.h"
#include "parser/cypher_parser.h"

/* Forward declarations */
void cypher_yyerror(CYPHER_YYLTYPE *yylloc, cypher_parser_context *context, const char *msg);
int cypher_yylex(CYPHER_YYSTYPE *yylval, CYPHER_YYLTYPE *yylloc, cypher_parser_context *context);


#line 84 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"

# ifndef YY_CAST
#  ifdef __cplusplus
#   define YY_CAST(Type, Val) static_cast<Type> (Val)
#   define YY_REINTERPRET_CAST(Type, Val) reinterpret_cast<Type> (Val)
#  else
#   define YY_CAST(Type, Val) ((Type) (Val))
#   define YY_REINTERPRET_CAST(Type, Val) ((Type) (Val))
#  endif
# endif
# ifndef YY_NULLPTR
#  if defined __cplusplus
#   if 201103L <= __cplusplus
#    define YY_NULLPTR nullptr
#   else
#    define YY_NULLPTR 0
#   endif
#  else
#   define YY_NULLPTR ((void*)0)
#  endif
# endif

#include "cypher_gram.tab.h"

/* Symbol kind.  */
enum yysymbol_kind_t
{
  YYSYMBOL_YYEMPTY = -2,
  YYSYMBOL_YYEOF = 0,                      /* "end of file"  */
  YYSYMBOL_YYerror = 1,                    /* error  */
  YYSYMBOL_YYUNDEF = 2,                    /* "invalid token"  */
  YYSYMBOL_INTEGER = 3,                    /* INTEGER  */
  YYSYMBOL_DECIMAL = 4,                    /* DECIMAL  */
  YYSYMBOL_STRING = 5,                     /* STRING  */
  YYSYMBOL_IDENTIFIER = 6,                 /* IDENTIFIER  */
  YYSYMBOL_PARAMETER = 7,                  /* PARAMETER  */
  YYSYMBOL_BQIDENT = 8,                    /* BQIDENT  */
  YYSYMBOL_NOT_EQ = 9,                     /* NOT_EQ  */
  YYSYMBOL_LT_EQ = 10,                     /* LT_EQ  */
  YYSYMBOL_GT_EQ = 11,                     /* GT_EQ  */
  YYSYMBOL_DOT_DOT = 12,                   /* DOT_DOT  */
  YYSYMBOL_TYPECAST = 13,                  /* TYPECAST  */
  YYSYMBOL_PLUS_EQ = 14,                   /* PLUS_EQ  */
  YYSYMBOL_REGEX_MATCH = 15,               /* REGEX_MATCH  */
  YYSYMBOL_MATCH = 16,                     /* MATCH  */
  YYSYMBOL_RETURN = 17,                    /* RETURN  */
  YYSYMBOL_CREATE = 18,                    /* CREATE  */
  YYSYMBOL_WHERE = 19,                     /* WHERE  */
  YYSYMBOL_WITH = 20,                      /* WITH  */
  YYSYMBOL_SET = 21,                       /* SET  */
  YYSYMBOL_DELETE = 22,                    /* DELETE  */
  YYSYMBOL_REMOVE = 23,                    /* REMOVE  */
  YYSYMBOL_MERGE = 24,                     /* MERGE  */
  YYSYMBOL_UNWIND = 25,                    /* UNWIND  */
  YYSYMBOL_DETACH = 26,                    /* DETACH  */
  YYSYMBOL_FOREACH = 27,                   /* FOREACH  */
  YYSYMBOL_CALL = 28,                      /* CALL  */
  YYSYMBOL_YIELD = 29,                     /* YIELD  */
  YYSYMBOL_OPTIONAL = 30,                  /* OPTIONAL  */
  YYSYMBOL_DISTINCT = 31,                  /* DISTINCT  */
  YYSYMBOL_ORDER = 32,                     /* ORDER  */
  YYSYMBOL_BY = 33,                        /* BY  */
  YYSYMBOL_SKIP = 34,                      /* SKIP  */
  YYSYMBOL_LIMIT = 35,                     /* LIMIT  */
  YYSYMBOL_AS = 36,                        /* AS  */
  YYSYMBOL_ASC = 37,                       /* ASC  */
  YYSYMBOL_DESC = 38,                      /* DESC  */
  YYSYMBOL_AND = 39,                       /* AND  */
  YYSYMBOL_OR = 40,                        /* OR  */
  YYSYMBOL_XOR = 41,                       /* XOR  */
  YYSYMBOL_NOT = 42,                       /* NOT  */
  YYSYMBOL_IN = 43,                        /* IN  */
  YYSYMBOL_IS = 44,                        /* IS  */
  YYSYMBOL_NULL_P = 45,                    /* NULL_P  */
  YYSYMBOL_TRUE_P = 46,                    /* TRUE_P  */
  YYSYMBOL_FALSE_P = 47,                   /* FALSE_P  */
  YYSYMBOL_EXISTS = 48,                    /* EXISTS  */
  YYSYMBOL_ANY = 49,                       /* ANY  */
  YYSYMBOL_NONE = 50,                      /* NONE  */
  YYSYMBOL_SINGLE = 51,                    /* SINGLE  */
  YYSYMBOL_REDUCE = 52,                    /* REDUCE  */
  YYSYMBOL_UNION = 53,                     /* UNION  */
  YYSYMBOL_ALL = 54,                       /* ALL  */
  YYSYMBOL_CASE = 55,                      /* CASE  */
  YYSYMBOL_WHEN = 56,                      /* WHEN  */
  YYSYMBOL_THEN = 57,                      /* THEN  */
  YYSYMBOL_ELSE = 58,                      /* ELSE  */
  YYSYMBOL_END_P = 59,                     /* END_P  */
  YYSYMBOL_ON = 60,                        /* ON  */
  YYSYMBOL_SHORTESTPATH = 61,              /* SHORTESTPATH  */
  YYSYMBOL_ALLSHORTESTPATHS = 62,          /* ALLSHORTESTPATHS  */
  YYSYMBOL_PATTERN = 63,                   /* PATTERN  */
  YYSYMBOL_EXPLAIN = 64,                   /* EXPLAIN  */
  YYSYMBOL_LOAD = 65,                      /* LOAD  */
  YYSYMBOL_CSV = 66,                       /* CSV  */
  YYSYMBOL_FROM = 67,                      /* FROM  */
  YYSYMBOL_HEADERS = 68,                   /* HEADERS  */
  YYSYMBOL_FIELDTERMINATOR = 69,           /* FIELDTERMINATOR  */
  YYSYMBOL_STARTS = 70,                    /* STARTS  */
  YYSYMBOL_ENDS = 71,                      /* ENDS  */
  YYSYMBOL_CONTAINS = 72,                  /* CONTAINS  */
  YYSYMBOL_73_ = 73,                       /* '='  */
  YYSYMBOL_74_ = 74,                       /* '<'  */
  YYSYMBOL_75_ = 75,                       /* '>'  */
  YYSYMBOL_76_ = 76,                       /* '+'  */
  YYSYMBOL_77_ = 77,                       /* '-'  */
  YYSYMBOL_78_ = 78,                       /* '*'  */
  YYSYMBOL_79_ = 79,                       /* '/'  */
  YYSYMBOL_80_ = 80,                       /* '%'  */
  YYSYMBOL_81_ = 81,                       /* '^'  */
  YYSYMBOL_82_ = 82,                       /* '.'  */
  YYSYMBOL_83_ = 83,                       /* '['  */
  YYSYMBOL_UNARY_MINUS = 84,               /* UNARY_MINUS  */
  YYSYMBOL_UNARY_PLUS = 85,                /* UNARY_PLUS  */
  YYSYMBOL_86_ = 86,                       /* ';'  */
  YYSYMBOL_87_ = 87,                       /* '('  */
  YYSYMBOL_88_ = 88,                       /* '|'  */
  YYSYMBOL_89_ = 89,                       /* ')'  */
  YYSYMBOL_90_ = 90,                       /* '{'  */
  YYSYMBOL_91_ = 91,                       /* '}'  */
  YYSYMBOL_92_ = 92,                       /* ','  */
  YYSYMBOL_93_ = 93,                       /* ':'  */
  YYSYMBOL_94_ = 94,                       /* ']'  */
  YYSYMBOL_YYACCEPT = 95,                  /* $accept  */
  YYSYMBOL_stmt = 96,                      /* stmt  */
  YYSYMBOL_union_query = 97,               /* union_query  */
  YYSYMBOL_single_query = 98,              /* single_query  */
  YYSYMBOL_clause_list = 99,               /* clause_list  */
  YYSYMBOL_clause = 100,                   /* clause  */
  YYSYMBOL_match_clause = 101,             /* match_clause  */
  YYSYMBOL_from_graph_opt = 102,           /* from_graph_opt  */
  YYSYMBOL_optional_opt = 103,             /* optional_opt  */
  YYSYMBOL_return_clause = 104,            /* return_clause  */
  YYSYMBOL_with_clause = 105,              /* with_clause  */
  YYSYMBOL_unwind_clause = 106,            /* unwind_clause  */
  YYSYMBOL_foreach_clause = 107,           /* foreach_clause  */
  YYSYMBOL_call_clause = 108,              /* call_clause  */
  YYSYMBOL_proc_args_opt = 109,            /* proc_args_opt  */
  YYSYMBOL_yield_item_list = 110,          /* yield_item_list  */
  YYSYMBOL_yield_item = 111,               /* yield_item  */
  YYSYMBOL_procedure_name = 112,           /* procedure_name  */
  YYSYMBOL_load_csv_clause = 113,          /* load_csv_clause  */
  YYSYMBOL_foreach_update_list = 114,      /* foreach_update_list  */
  YYSYMBOL_distinct_opt = 115,             /* distinct_opt  */
  YYSYMBOL_order_by_opt = 116,             /* order_by_opt  */
  YYSYMBOL_skip_opt = 117,                 /* skip_opt  */
  YYSYMBOL_limit_opt = 118,                /* limit_opt  */
  YYSYMBOL_where_opt = 119,                /* where_opt  */
  YYSYMBOL_order_by_list = 120,            /* order_by_list  */
  YYSYMBOL_order_by_item = 121,            /* order_by_item  */
  YYSYMBOL_return_item_list = 122,         /* return_item_list  */
  YYSYMBOL_return_item = 123,              /* return_item  */
  YYSYMBOL_set_item_list = 124,            /* set_item_list  */
  YYSYMBOL_set_target = 125,               /* set_target  */
  YYSYMBOL_set_item = 126,                 /* set_item  */
  YYSYMBOL_create_clause = 127,            /* create_clause  */
  YYSYMBOL_merge_clause = 128,             /* merge_clause  */
  YYSYMBOL_on_create_clause = 129,         /* on_create_clause  */
  YYSYMBOL_on_match_clause = 130,          /* on_match_clause  */
  YYSYMBOL_set_clause = 131,               /* set_clause  */
  YYSYMBOL_delete_clause = 132,            /* delete_clause  */
  YYSYMBOL_delete_item_list = 133,         /* delete_item_list  */
  YYSYMBOL_delete_item = 134,              /* delete_item  */
  YYSYMBOL_remove_clause = 135,            /* remove_clause  */
  YYSYMBOL_remove_item_list = 136,         /* remove_item_list  */
  YYSYMBOL_remove_item = 137,              /* remove_item  */
  YYSYMBOL_detach_opt = 138,               /* detach_opt  */
  YYSYMBOL_pattern_list = 139,             /* pattern_list  */
  YYSYMBOL_simple_path = 140,              /* simple_path  */
  YYSYMBOL_path = 141,                     /* path  */
  YYSYMBOL_node_pattern = 142,             /* node_pattern  */
  YYSYMBOL_rel_pattern = 143,              /* rel_pattern  */
  YYSYMBOL_variable_opt = 144,             /* variable_opt  */
  YYSYMBOL_varlen_range_opt = 145,         /* varlen_range_opt  */
  YYSYMBOL_label_opt = 146,                /* label_opt  */
  YYSYMBOL_label_name = 147,               /* label_name  */
  YYSYMBOL_label_alt_list = 148,           /* label_alt_list  */
  YYSYMBOL_label_list = 149,               /* label_list  */
  YYSYMBOL_rel_type_list = 150,            /* rel_type_list  */
  YYSYMBOL_expr = 151,                     /* expr  */
  YYSYMBOL_primary_expr = 152,             /* primary_expr  */
  YYSYMBOL_literal_expr = 153,             /* literal_expr  */
  YYSYMBOL_function_call = 154,            /* function_call  */
  YYSYMBOL_list_predicate = 155,           /* list_predicate  */
  YYSYMBOL_reduce_expr = 156,              /* reduce_expr  */
  YYSYMBOL_argument_list = 157,            /* argument_list  */
  YYSYMBOL_list_literal = 158,             /* list_literal  */
  YYSYMBOL_list_comprehension = 159,       /* list_comprehension  */
  YYSYMBOL_pattern_comprehension = 160,    /* pattern_comprehension  */
  YYSYMBOL_map_literal = 161,              /* map_literal  */
  YYSYMBOL_map_projection = 162,           /* map_projection  */
  YYSYMBOL_map_projection_list = 163,      /* map_projection_list  */
  YYSYMBOL_map_projection_item = 164,      /* map_projection_item  */
  YYSYMBOL_case_expression = 165,          /* case_expression  */
  YYSYMBOL_when_clause_list = 166,         /* when_clause_list  */
  YYSYMBOL_when_clause = 167,              /* when_clause  */
  YYSYMBOL_literal = 168,                  /* literal  */
  YYSYMBOL_identifier = 169,               /* identifier  */
  YYSYMBOL_non_reserved_kw = 170,          /* non_reserved_kw  */
  YYSYMBOL_parameter = 171,                /* parameter  */
  YYSYMBOL_properties_opt = 172,           /* properties_opt  */
  YYSYMBOL_map_pair_list = 173,            /* map_pair_list  */
  YYSYMBOL_map_pair = 174                  /* map_pair  */
};
typedef enum yysymbol_kind_t yysymbol_kind_t;


/* Default (constant) value used for initialization for null
   right-hand sides.  Unlike the standard yacc.c template, here we set
   the default value of $$ to a zeroed-out value.  Since the default
   value is undefined, this behavior is technically correct.  */
static YYSTYPE yyval_default;
static YYLTYPE yyloc_default
# if defined CYPHER_YYLTYPE_IS_TRIVIAL && CYPHER_YYLTYPE_IS_TRIVIAL
  = { 1, 1, 1, 1 }
# endif
;


/* Unqualified %code blocks.  */
#line 26 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"

/* The source text from (line, col) to (last_line, last_col), both
 * 1-based and inclusive, trimmed. NULL when the input cannot be read. */
static char *source_text_span(cypher_parser_context *context, int line, int col,
                              int last_line, int last_col)
{
    const char *in = (context && context->scanner) ? context->scanner->input_string : NULL;
    if (!in || line < 1 || col < 1 || last_line < line) return NULL;
    const char *p = in;
    for (int l = 1; l < line && *p; p++) if (*p == '\n') l++;
    for (int c = 1; c < col && *p && *p != '\n'; c++) p++;
    const char *q = p;
    int l = line, c = col;
    while (*q && (l < last_line || (l == last_line && c < last_col))) {
        if (*q == '\n') { l++; c = 1; } else c++;
        q++;
    }
    if (*q) q++;   /* inclusive end */
    while (p < q && (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r')) p++;
    while (q > p && (q[-1] == ' ' || q[-1] == '\t' || q[-1] == '\n' || q[-1] == '\r')) q--;
    if (q <= p) return NULL;
    return strndup(p, (size_t)(q - p));
}

/* The spelling of a keyword as written, at `line` / `col` (1-based) of the
 * input, for a keyword used as a map key: `{NULL: 1}` keeps the key "NULL"
 * (the scanner upper-cases keywords; Map1 [5], Map2 [5]). `fallback` when
 * the input cannot be read. Owned by the caller. */
static char *keyword_text_at(cypher_parser_context *context, int line, int col,
                             const char *fallback)
{
    const char *in = (context && context->scanner) ? context->scanner->input_string : NULL;
    if (!in || line < 1 || col < 1) return strdup(fallback);
    for (int l = 1; l < line && *in; in++) if (*in == '\n') l++;
    for (int c = 1; c < col && *in && *in != '\n'; c++) in++;
    size_t n = 0;
    while (in[n] && ((in[n] >= 'A' && in[n] <= 'Z') || (in[n] >= 'a' && in[n] <= 'z') ||
                     in[n] == '_' || (in[n] >= '0' && in[n] <= '9'))) n++;
    if (n == 0 || strncasecmp(in, fallback, n) != 0 || strlen(fallback) != n) return strdup(fallback);
    return strndup(in, n);
}

#line 348 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef short
# undef short
#endif

/* On compilers that do not define __PTRDIFF_MAX__ etc., make sure
   <limits.h> and (if available) <stdint.h> are included
   so that the code can choose integer types of a good width.  */

#ifndef __PTRDIFF_MAX__
# include <limits.h> /* INFRINGES ON USER NAME SPACE */
# if defined __STDC_VERSION__ && 199901 <= __STDC_VERSION__
#  include <stdint.h> /* INFRINGES ON USER NAME SPACE */
#  define YY_STDINT_H
# endif
#endif

/* Narrow types that promote to a signed type and that can represent a
   signed or unsigned integer of at least N bits.  In tables they can
   save space and decrease cache pressure.  Promoting to a signed type
   helps avoid bugs in integer arithmetic.  */

#ifdef __INT_LEAST8_MAX__
typedef __INT_LEAST8_TYPE__ yytype_int8;
#elif defined YY_STDINT_H
typedef int_least8_t yytype_int8;
#else
typedef signed char yytype_int8;
#endif

#ifdef __INT_LEAST16_MAX__
typedef __INT_LEAST16_TYPE__ yytype_int16;
#elif defined YY_STDINT_H
typedef int_least16_t yytype_int16;
#else
typedef short yytype_int16;
#endif

/* Work around bug in HP-UX 11.23, which defines these macros
   incorrectly for preprocessor constants.  This workaround can likely
   be removed in 2023, as HPE has promised support for HP-UX 11.23
   (aka HP-UX 11i v2) only through the end of 2022; see Table 2 of
   <https://h20195.www2.hpe.com/V2/getpdf.aspx/4AA4-7673ENW.pdf>.  */
#ifdef __hpux
# undef UINT_LEAST8_MAX
# undef UINT_LEAST16_MAX
# define UINT_LEAST8_MAX 255
# define UINT_LEAST16_MAX 65535
#endif

#if defined __UINT_LEAST8_MAX__ && __UINT_LEAST8_MAX__ <= __INT_MAX__
typedef __UINT_LEAST8_TYPE__ yytype_uint8;
#elif (!defined __UINT_LEAST8_MAX__ && defined YY_STDINT_H \
       && UINT_LEAST8_MAX <= INT_MAX)
typedef uint_least8_t yytype_uint8;
#elif !defined __UINT_LEAST8_MAX__ && UCHAR_MAX <= INT_MAX
typedef unsigned char yytype_uint8;
#else
typedef short yytype_uint8;
#endif

#if defined __UINT_LEAST16_MAX__ && __UINT_LEAST16_MAX__ <= __INT_MAX__
typedef __UINT_LEAST16_TYPE__ yytype_uint16;
#elif (!defined __UINT_LEAST16_MAX__ && defined YY_STDINT_H \
       && UINT_LEAST16_MAX <= INT_MAX)
typedef uint_least16_t yytype_uint16;
#elif !defined __UINT_LEAST16_MAX__ && USHRT_MAX <= INT_MAX
typedef unsigned short yytype_uint16;
#else
typedef int yytype_uint16;
#endif
#ifndef YYPTRDIFF_T
# if defined __PTRDIFF_TYPE__ && defined __PTRDIFF_MAX__
#  define YYPTRDIFF_T __PTRDIFF_TYPE__
#  define YYPTRDIFF_MAXIMUM __PTRDIFF_MAX__
# elif defined PTRDIFF_MAX
#  ifndef ptrdiff_t
#   include <stddef.h> /* INFRINGES ON USER NAME SPACE */
#  endif
#  define YYPTRDIFF_T ptrdiff_t
#  define YYPTRDIFF_MAXIMUM PTRDIFF_MAX
# else
#  define YYPTRDIFF_T long
#  define YYPTRDIFF_MAXIMUM LONG_MAX
# endif
#endif

#ifndef YYSIZE_T
# ifdef __SIZE_TYPE__
#  define YYSIZE_T __SIZE_TYPE__
# elif defined size_t
#  define YYSIZE_T size_t
# elif defined __STDC_VERSION__ && 199901 <= __STDC_VERSION__
#  include <stddef.h> /* INFRINGES ON USER NAME SPACE */
#  define YYSIZE_T size_t
# else
#  define YYSIZE_T unsigned
# endif
#endif

#define YYSIZE_MAXIMUM                                  \
  YY_CAST (YYPTRDIFF_T,                                 \
           (YYPTRDIFF_MAXIMUM < YY_CAST (YYSIZE_T, -1)  \
            ? YYPTRDIFF_MAXIMUM                         \
            : YY_CAST (YYSIZE_T, -1)))

#define YYSIZEOF(X) YY_CAST (YYPTRDIFF_T, sizeof (X))


#ifndef YY_
# if defined YYENABLE_NLS && YYENABLE_NLS
#  if ENABLE_NLS
#   include <libintl.h> /* INFRINGES ON USER NAME SPACE */
#   define YY_(Msgid) dgettext ("bison-runtime", Msgid)
#  endif
# endif
# ifndef YY_
#  define YY_(Msgid) Msgid
# endif
#endif


#ifndef YYFREE
# define YYFREE free
#endif
#ifndef YYMALLOC
# define YYMALLOC malloc
#endif
#ifndef YYREALLOC
# define YYREALLOC realloc
#endif

#ifdef __cplusplus
  typedef bool yybool;
# define yytrue true
# define yyfalse false
#else
  /* When we move to stdbool, get rid of the various casts to yybool.  */
  typedef signed char yybool;
# define yytrue 1
# define yyfalse 0
#endif

#ifndef YYSETJMP
# include <setjmp.h>
# define YYJMP_BUF jmp_buf
# define YYSETJMP(Env) setjmp (Env)
/* Pacify Clang and ICC.  */
# define YYLONGJMP(Env, Val)                    \
 do {                                           \
   longjmp (Env, Val);                          \
   YY_ASSERT (0);                               \
 } while (yyfalse)
#endif

#ifndef YY_ATTRIBUTE_PURE
# if defined __GNUC__ && 2 < __GNUC__ + (96 <= __GNUC_MINOR__)
#  define YY_ATTRIBUTE_PURE __attribute__ ((__pure__))
# else
#  define YY_ATTRIBUTE_PURE
# endif
#endif

#ifndef YY_ATTRIBUTE_UNUSED
# if defined __GNUC__ && 2 < __GNUC__ + (7 <= __GNUC_MINOR__)
#  define YY_ATTRIBUTE_UNUSED __attribute__ ((__unused__))
# else
#  define YY_ATTRIBUTE_UNUSED
# endif
#endif

/* The _Noreturn keyword of C11.  */
#ifndef _Noreturn
# if (defined __cplusplus \
      && ((201103 <= __cplusplus && !(__GNUC__ == 4 && __GNUC_MINOR__ == 7)) \
          || (defined _MSC_VER && 1900 <= _MSC_VER)))
#  define _Noreturn [[noreturn]]
# elif ((!defined __cplusplus || defined __clang__) \
        && (201112 <= (defined __STDC_VERSION__ ? __STDC_VERSION__ : 0) \
            || (!defined __STRICT_ANSI__ \
                && (4 < __GNUC__ + (7 <= __GNUC_MINOR__) \
                    || (defined __apple_build_version__ \
                        ? 6000000 <= __apple_build_version__ \
                        : 3 < __clang_major__ + (5 <= __clang_minor__))))))
   /* _Noreturn works as-is.  */
# elif (2 < __GNUC__ + (8 <= __GNUC_MINOR__) || defined __clang__ \
        || 0x5110 <= __SUNPRO_C)
#  define _Noreturn __attribute__ ((__noreturn__))
# elif 1200 <= (defined _MSC_VER ? _MSC_VER : 0)
#  define _Noreturn __declspec (noreturn)
# else
#  define _Noreturn
# endif
#endif

/* Suppress unused-variable warnings by "using" E.  */
#if ! defined lint || defined __GNUC__
# define YY_USE(E) ((void) (E))
#else
# define YY_USE(E) /* empty */
#endif

/* Suppress an incorrect diagnostic about yylval being uninitialized.  */
#if defined __GNUC__ && ! defined __ICC && 406 <= __GNUC__ * 100 + __GNUC_MINOR__
# if __GNUC__ * 100 + __GNUC_MINOR__ < 407
#  define YY_IGNORE_MAYBE_UNINITIALIZED_BEGIN                           \
    _Pragma ("GCC diagnostic push")                                     \
    _Pragma ("GCC diagnostic ignored \"-Wuninitialized\"")
# else
#  define YY_IGNORE_MAYBE_UNINITIALIZED_BEGIN                           \
    _Pragma ("GCC diagnostic push")                                     \
    _Pragma ("GCC diagnostic ignored \"-Wuninitialized\"")              \
    _Pragma ("GCC diagnostic ignored \"-Wmaybe-uninitialized\"")
# endif
# define YY_IGNORE_MAYBE_UNINITIALIZED_END      \
    _Pragma ("GCC diagnostic pop")
#else
# define YY_INITIAL_VALUE(Value) Value
#endif
#ifndef YY_IGNORE_MAYBE_UNINITIALIZED_BEGIN
# define YY_IGNORE_MAYBE_UNINITIALIZED_BEGIN
# define YY_IGNORE_MAYBE_UNINITIALIZED_END
#endif
#ifndef YY_INITIAL_VALUE
# define YY_INITIAL_VALUE(Value) /* Nothing. */
#endif

#if defined __cplusplus && defined __GNUC__ && ! defined __ICC && 6 <= __GNUC__
# define YY_IGNORE_USELESS_CAST_BEGIN                          \
    _Pragma ("GCC diagnostic push")                            \
    _Pragma ("GCC diagnostic ignored \"-Wuseless-cast\"")
# define YY_IGNORE_USELESS_CAST_END            \
    _Pragma ("GCC diagnostic pop")
#endif
#ifndef YY_IGNORE_USELESS_CAST_BEGIN
# define YY_IGNORE_USELESS_CAST_BEGIN
# define YY_IGNORE_USELESS_CAST_END
#endif


#define YY_ASSERT(E) ((void) (0 && (E)))

/* YYFINAL -- State number of the termination state.  */
#define YYFINAL  107
/* YYLAST -- Last index in YYTABLE.  */
#define YYLAST   3728

/* YYNTOKENS -- Number of terminals.  */
#define YYNTOKENS  95
/* YYNNTS -- Number of nonterminals.  */
#define YYNNTS  80
/* YYNRULES -- Number of rules.  */
#define YYNRULES  373
/* YYNSTATES -- Number of states.  */
#define YYNSTATES  755
/* YYMAXRHS -- Maximum number of symbols on right-hand side of rule.  */
#define YYMAXRHS 15
/* YYMAXLEFT -- Maximum number of symbols to the left of a handle
   accessed by $0, $-1, etc., in any rule.  */
#define YYMAXLEFT 0

/* YYMAXUTOK -- Last valid token kind.  */
#define YYMAXUTOK   329

/* YYTRANSLATE(TOKEN-NUM) -- Symbol number corresponding to TOKEN-NUM
   as returned by yylex, with out-of-bounds checking.  */
#define YYTRANSLATE(YYX)                                \
  (0 <= (YYX) && (YYX) <= YYMAXUTOK                     \
   ? YY_CAST (yysymbol_kind_t, yytranslate[YYX])        \
   : YYSYMBOL_YYUNDEF)

/* YYTRANSLATE[TOKEN-NUM] -- Symbol number corresponding to TOKEN-NUM
   as returned by yylex.  */
static const yytype_int8 yytranslate[] =
{
       0,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,    80,     2,     2,
      87,    89,    78,    76,    92,    77,    82,    79,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,    93,    86,
      74,    73,    75,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,    83,     2,    94,    81,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,    90,    88,    91,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     1,     2,     3,     4,
       5,     6,     7,     8,     9,    10,    11,    12,    13,    14,
      15,    16,    17,    18,    19,    20,    21,    22,    23,    24,
      25,    26,    27,    28,    29,    30,    31,    32,    33,    34,
      35,    36,    37,    38,    39,    40,    41,    42,    43,    44,
      45,    46,    47,    48,    49,    50,    51,    52,    53,    54,
      55,    56,    57,    58,    59,    60,    61,    62,    63,    64,
      65,    66,    67,    68,    69,    70,    71,    72,    84,    85
};

#if CYPHER_YYDEBUG
/* YYRLINE[YYN] -- source line where rule number YYN was defined.  */
static const yytype_int16 yyrline[] =
{
       0,   220,   220,   225,   230,   239,   250,   254,   258,   265,
     272,   277,   285,   286,   287,   288,   289,   290,   291,   292,
     293,   294,   295,   296,   301,   308,   309,   316,   317,   322,
     326,   331,   339,   343,   352,   357,   367,   372,   386,   392,
     399,   404,   409,   417,   418,   419,   423,   428,   436,   441,
     450,   452,   464,   470,   476,   483,   494,   499,   504,   509,
     514,   519,   524,   529,   534,   539,   544,   549,   557,   558,
     562,   563,   567,   568,   572,   573,   577,   578,   582,   587,
     595,   596,   597,   602,   607,   615,   623,   628,   639,   644,
     649,   665,   692,   693,   694,   699,   704,   712,   716,   721,
     734,   742,   746,   750,   754,   758,   765,   772,   780,   788,
     795,   800,   808,   817,   824,   829,   834,   850,   874,   883,
     892,   901,   910,   922,   927,   934,   939,   948,   954,   973,
     977,   983,   989,   996,  1004,  1009,  1016,  1024,  1032,  1047,
    1051,  1056,  1061,  1067,  1074,  1078,  1083,  1088,  1094,  1103,
    1107,  1112,  1117,  1122,  1129,  1133,  1138,  1143,  1149,  1156,
    1160,  1164,  1170,  1177,  1178,  1179,  1180,  1186,  1187,  1189,
    1191,  1193,  1195,  1197,  1199,  1213,  1214,  1215,  1226,  1227,
    1228,  1232,  1239,  1248,  1255,  1262,  1269,  1276,  1283,  1293,
    1303,  1314,  1324,  1335,  1345,  1356,  1366,  1377,  1384,  1391,
    1398,  1409,  1410,  1411,  1440,  1441,  1442,  1443,  1444,  1445,
    1446,  1447,  1450,  1451,  1452,  1453,  1454,  1455,  1456,  1457,
    1458,  1459,  1460,  1461,  1462,  1463,  1464,  1465,  1471,  1484,
    1498,  1503,  1508,  1513,  1517,  1521,  1525,  1532,  1533,  1534,
    1535,  1536,  1537,  1538,  1539,  1540,  1541,  1542,  1543,  1544,
    1551,  1564,  1581,  1608,  1615,  1625,  1636,  1651,  1677,  1684,
    1692,  1697,  1707,  1714,  1725,  1735,  1739,  1744,  1753,  1758,
    1763,  1768,  1777,  1788,  1793,  1802,  1806,  1823,  1828,  1833,
    1838,  1855,  1868,  1882,  1897,  1916,  1920,  1928,  1937,  1942,
    1950,  1956,  1961,  1968,  1974,  1989,  1993,  1998,  2002,  2009,
    2014,  2022,  2029,  2033,  2037,  2042,  2046,  2050,  2057,  2062,
    2067,  2079,  2080,  2081,  2082,  2083,  2084,  2085,  2086,  2087,
    2088,  2089,  2090,  2091,  2092,  2093,  2094,  2095,  2096,  2097,
    2098,  2099,  2100,  2101,  2102,  2103,  2104,  2105,  2106,  2107,
    2108,  2109,  2110,  2111,  2112,  2113,  2114,  2115,  2116,  2117,
    2118,  2119,  2120,  2121,  2122,  2123,  2124,  2125,  2126,  2127,
    2131,  2140,  2141,  2148,  2155,  2160,  2168,  2172,  2176,  2180,
    2184,  2188,  2195,  2201
};
#endif

#define YYPACT_NINF (-523)
#define YYTABLE_NINF (-181)

/* YYPACT[STATE-NUM] -- Index in YYTABLE of the portion describing
   STATE-NUM.  */
static const yytype_int16 yypact[] =
{
     625,     8,    85,    22,    62,   174,    85,   944,  -523,   -44,
      20,  -523,  2450,   -14,    77,   -19,  -523,   724,  -523,  -523,
     147,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,
    -523,  -523,    63,    65,   167,   944,   236,   250,   198,   255,
      42,   254,   202,  -523,  -523,  -523,   167,   944,   285,  -523,
    -523,   944,   301,   151,  -523,  -523,    94,   321,   320,  -523,
      79,  -523,  -523,  -523,   312,  -523,   944,  -523,  -523,  -523,
     224,   335,   354,   376,   399,   413,   622,   427,   436,   459,
     944,   944,   212,  1001,  1134,   202,  2521,  -523,  -523,  -523,
    -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,
    -523,   245,  -523,  2450,   210,    -2,    35,  -523,  3104,  -523,
    -523,    85,   944,   167,   478,   484,     6,  -523,  2569,   127,
     216,   464,   464,  -523,  -523,  -523,   460,    85,   475,   -12,
     464,   484,     6,   548,  1861,    91,   944,   944,  3254,   332,
     549,   362,   405,   476,   496,   497,   552,   531,    26,   554,
    3108,    98,   304,   555,   557,   558,   559,   560,   944,  2614,
      86,  -523,   944,   944,   944,  -523,  -523,   314,  1058,  -523,
     418,   211,   358,   360,  1908,   474,   477,   479,  -523,  -523,
    -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,
    -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,   481,
     482,  -523,  -523,  -523,  -523,  -523,  -523,  -523,   491,   494,
    -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,
    -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,
    -523,  -523,   495,   306,  -523,   464,   944,   944,   944,   944,
     509,   944,   944,   944,   944,   291,   551,   569,   944,   944,
     944,   944,   944,   944,   944,   944,   944,   944,  3321,   716,
     525,   526,    -8,   585,   773,   563,  -523,   528,   593,  2450,
    -523,   -30,   507,  -523,  2977,   484,   944,   944,   565,   944,
     484,   513,   517,   518,   202,   523,   524,   202,   -28,   107,
    3388,   522,   520,  -523,   144,   540,    42,  -523,   565,   484,
     529,  -523,   530,  -523,  2977,  2977,  -523,  -523,  -523,  -523,
    -523,   538,  -523,  -523,    97,  -523,   595,   596,   603,  -523,
     606,  -523,   545,   944,   544,  -523,  2977,   275,   541,    25,
     336,  -523,   542,   183,   303,   546,    41,   597,   601,   611,
     566,   614,  2662,   318,   944,  -523,  -523,   373,   390,   395,
     944,   571,   460,  -523,  -523,   944,   944,   944,   944,   944,
     944,   944,   944,  -523,  3187,   414,   338,   338,   338,   338,
    -523,  -523,  3108,  3022,  3067,   450,   591,  -523,   944,   944,
     450,   338,   338,   338,   357,   357,   302,   302,   302,   424,
    -523,  -523,  -523,   944,  1320,   944,   944,  -523,  -523,  -523,
     401,    60,   592,   602,  -523,   654,   642,   944,   565,   570,
    -523,  2473,  2977,   944,  -523,  -523,   565,  -523,  -523,   464,
     464,   464,   464,  -523,  -523,   575,   577,   578,   587,   594,
    1210,   590,  3455,   608,    42,  -523,   145,   642,   565,   678,
     679,   680,   681,    62,    62,   830,  1955,  -523,  -523,   944,
     887,  -523,  -523,  -523,    26,   682,   685,  -523,  -523,   944,
    -523,   944,   944,   944,   944,   944,   944,   944,  -523,  2707,
    -523,  -523,  -523,  1274,    42,   522,  2977,  2977,  2977,  2977,
    2977,  2977,  2977,  2977,  -523,   464,  -523,   450,   450,  1411,
     426,  -523,  2237,  2283,  -523,   659,  -523,     2,  -523,   691,
     694,  -523,   944,  -523,  -523,  -523,   944,  -523,  -523,  2977,
    -523,   109,   270,   284,   286,  3522,  3522,  -523,   453,  -523,
    -523,  -523,  -523,  -523,   162,    75,  3589,   522,  -523,   642,
     604,  -523,   609,  -523,   301,   301,  -523,   412,  -523,  2977,
     695,  2977,  -523,   610,   615,  1816,  2752,  2797,  2842,  1771,
    2887,  2977,  2932,  -523,   944,   944,  -523,   460,   619,  -523,
    -523,  -523,  1456,   443,   443,   704,   944,   705,   677,   645,
    2977,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,  -523,
    -523,  -523,  3656,   522,   703,   714,   722,   -31,   123,   227,
     648,   633,  -523,   723,   728,  -523,  -523,   729,  -523,  -523,
     944,   944,   944,   730,   944,  -523,  1365,  1501,   522,   431,
    -523,  -523,   179,  -523,  -523,  -523,  -523,  -523,   186,  -523,
    2977,  -523,   731,   725,   -31,   123,   227,   648,   644,   736,
    -523,  -523,    16,   522,    21,   522,    27,   522,   522,   676,
    -523,  -523,   662,  2002,  2049,  2096,   713,  2143,   944,  -523,
    -523,   668,   464,  -523,  -523,  -523,  -523,  -523,  -523,  -523,
    -523,   690,  -523,   522,   522,   522,   522,   683,  -523,  -523,
    -523,   516,   675,  -523,  -523,   534,   688,  -523,  -523,   535,
     689,   696,   697,   767,  -523,  -523,  -523,   944,  -523,  1546,
     202,    53,   769,   700,   701,   702,   706,   709,  -523,  -523,
     708,  -523,  -523,   720,  -523,  -523,   721,   727,  -523,  -523,
    2329,  -523,   464,   944,   944,  -523,   732,   733,   734,   735,
    -523,   726,   738,   739,   741,   944,    69,  2375,  1591,   742,
     751,   754,   755,  -523,  -523,  -523,  -523,  2190,   944,   944,
     944,  -523,  -523,  -523,  -523,  -523,  -523,  2421,  1636,  1681,
     944,  -523,  -523,  1726,  -523
};

/* YYDEFACT[STATE-NUM] -- Default reduction number in state STATE-NUM.
   Performed when YYTABLE does not specify something else to do.  Zero
   means the default is an error.  */
static const yytype_int16 yydefact[] =
{
      27,    68,     0,    68,     0,     0,     0,     0,   123,     0,
       0,    28,    27,     0,     0,     2,     6,     9,    10,    12,
       0,    13,    14,    15,    16,    23,    17,    18,    19,    20,
      21,    22,     0,    69,    70,     0,     0,     0,     0,     0,
     163,   100,   129,   125,   127,    69,    70,     0,   308,   309,
     310,     0,   108,     0,    88,    92,     0,     0,   113,   114,
     101,   302,   303,   304,   308,   360,     0,   307,   305,   306,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,   163,     0,     0,     0,   201,   237,   240,
     241,   242,   243,   244,   245,   246,   247,   248,   253,   238,
     239,     0,    50,    27,    43,     4,     0,     1,    27,     3,
      11,     0,     0,    70,     0,    72,    70,    83,    85,     0,
       0,     0,     0,   164,   165,   166,   175,     0,     0,     0,
       0,    72,    70,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,   102,   103,     0,     0,     0,     0,
     224,     0,    27,     0,     0,     0,     0,     0,     0,     0,
       0,   299,     0,     0,     0,   202,   203,   308,   163,   275,
       0,   308,   309,   310,     0,     0,     0,     0,   340,   341,
     342,   343,   344,   345,   346,   347,   348,   349,   350,   351,
     352,   325,   353,   354,   333,   334,   355,   356,   328,     0,
       0,   330,   329,   331,   332,   326,   327,   335,     0,     0,
     315,   312,   313,   311,   319,   357,   314,   336,   337,   338,
     339,   320,   321,   322,   324,   323,   358,   359,   317,   318,
     316,   285,     0,     0,   364,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,    39,     5,     0,     0,    27,
       7,    25,   109,   110,   112,    72,     0,     0,    74,     0,
      72,     0,     0,     0,   130,     0,     0,   131,     0,     0,
       0,   361,   176,   126,     0,   162,   163,   128,    74,    72,
      99,    93,   308,    89,    98,    97,    94,    95,    96,   118,
     119,   122,   120,   121,     0,   115,     0,     0,     0,   104,
       0,   105,     0,     0,     0,   256,   273,     0,   294,     0,
       0,   288,   249,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,   295,   300,     0,     0,     0,
       0,     0,   175,   276,   227,     0,     0,     0,     0,     0,
       0,     0,     0,   286,     0,   228,   211,   214,   215,   216,
      34,    35,   217,   218,   219,   220,     0,   225,     0,     0,
     223,   210,   212,   213,   204,   205,   206,   207,   208,   209,
     230,   231,   232,     0,     0,     0,     0,    38,    51,    44,
       0,     0,     0,     0,     8,     0,    76,     0,    74,    71,
      78,    80,    73,     0,    30,    84,    74,    86,    87,     0,
       0,     0,     0,   134,   137,   183,   184,     0,   177,   185,
       0,     0,     0,   160,   163,   159,   167,    76,    74,     0,
       0,     0,     0,     0,     0,     0,     0,   259,   257,     0,
       0,   290,   291,   287,     0,     0,     0,   260,   262,     0,
     261,     0,     0,     0,     0,     0,     0,     0,   297,     0,
     266,   267,   265,     0,   163,   361,   368,   366,   367,   370,
     373,   371,   372,   369,   365,     0,   226,   221,   222,     0,
       0,   233,     0,     0,    45,    48,    40,    41,    46,     0,
       0,    26,     0,    24,   111,    31,     0,    81,    82,    75,
      29,     0,     0,     0,     0,     0,     0,   362,     0,   138,
     186,   188,   187,   161,   167,   168,     0,   361,    33,    76,
      90,    99,   116,   122,   107,   106,   255,     0,   258,   274,
       0,   293,   289,   250,     0,     0,     0,     0,     0,     0,
       0,   301,     0,   296,     0,     0,   277,   175,     0,   229,
     236,   235,     0,   124,   124,     0,     0,     0,     0,    52,
      77,    79,   132,   135,   133,   136,   178,   179,   181,   180,
     182,   363,     0,   361,   169,   173,     0,   167,   167,   167,
     167,     0,    32,     0,     0,   254,   292,     0,   264,   263,
       0,     0,     0,     0,     0,   298,     0,     0,   361,     0,
     234,    61,   124,    56,    59,    57,    58,    60,   124,    49,
      42,    47,     0,     0,   167,   167,   167,   167,     0,   171,
     172,   174,     0,   361,     0,   361,     0,   361,   361,     0,
      91,   117,   251,     0,     0,     0,     0,     0,     0,   278,
     279,     0,     0,    36,    67,    62,    65,    63,    64,    66,
      37,    53,    54,   361,   361,   361,   361,     0,   170,   189,
     191,     0,     0,   193,   195,     0,     0,   197,   198,     0,
       0,     0,   154,     0,   269,   270,   271,     0,   268,     0,
       0,     0,     0,     0,     0,     0,     0,   144,   190,   192,
       0,   194,   196,     0,   199,   200,     0,     0,   139,   252,
       0,   280,     0,     0,     0,    55,     0,     0,     0,     0,
     149,   155,   156,   158,   157,     0,     0,     0,     0,   145,
     146,   148,   147,   140,   141,   143,   142,     0,     0,     0,
       0,   281,   150,   151,   153,   152,   272,     0,     0,     0,
       0,   283,   282,     0,   284
};

/* YYPGOTO[NTERM-NUM].  */
static const yytype_int16 yypgoto[] =
{
    -523,  -523,    24,   -94,  -523,   774,  -523,  -523,  -523,  -523,
    -523,  -523,  -522,  -523,  -523,  -523,   235,  -523,  -523,   241,
     804,   -21,  -122,  -268,  -422,  -523,   325,     7,   561,  -380,
    -523,   673,  -390,  -338,   707,   698,  -277,  -273,  -523,   432,
    -256,  -523,   699,  -523,    17,  -102,   719,     4,   -84,  -161,
    -508,  -349,    34,  -523,  -523,   265,    -7,  -523,  -523,  -523,
    -523,  -523,  -151,  -523,  -523,  -523,  -523,  -523,  -523,   394,
    -523,   692,  -152,  -523,     1,  -136,  -523,  -471,   423,   490
};

/* YYDEFGOTO[NTERM-NUM].  */
static const yytype_int16 yydefgoto[] =
{
       0,    14,    15,    16,    17,    18,    19,   406,    20,    21,
      22,    23,    24,    25,   265,   497,   498,   104,    26,   612,
      35,   115,   278,   414,   503,   409,   410,   116,   117,    52,
      53,    54,    27,    28,   144,   145,    29,    30,   272,   273,
      31,    58,    59,    32,    41,    42,    43,    85,   130,   126,
     527,   291,   427,   428,   292,   589,   326,    87,    88,    89,
      90,    91,   327,    92,    93,    94,    95,    96,   330,   331,
      97,   160,   161,    98,    99,   232,   100,   431,   233,   234
};

/* YYTABLE[YYPACT[STATE-NUM]] -- What to do in state STATE-NUM.  If
   positive, shift that token.  If negative, reduce the rule whose
   number is the opposite.  If YYTABLE_NINF, syntax error.  */
static const yytype_int16 yytable[] =
{
      86,   235,   308,   475,   558,    55,    44,   352,   346,   298,
      44,   347,   348,   349,   270,   528,   583,   284,   287,   288,
     289,   566,   669,    60,   670,   131,   102,   673,   118,   674,
     437,   451,   328,   677,   108,   678,   105,   405,   114,    33,
     118,   611,   611,   101,   134,   108,   128,   525,   123,   129,
     124,   108,   106,    45,   132,   267,   591,   632,   335,   150,
     459,   423,   127,   534,   535,   295,   495,   109,    48,   159,
      49,   296,   713,   165,   166,   118,   174,   107,   584,   633,
     635,   637,   638,   397,   266,   112,    34,   585,   738,   170,
     654,    36,   275,    37,   567,   280,   654,   302,   279,    49,
      46,   125,   268,   452,   333,   274,    37,   592,   329,   671,
     103,   299,   628,   400,   675,    44,   663,   664,   665,   666,
     679,    50,   392,    44,    44,    44,    44,   262,   271,   304,
     305,    44,   460,   127,   297,   436,    55,   651,   496,   143,
     505,   714,   158,   113,   344,   345,    38,    39,   510,    51,
      50,   342,   586,   408,   429,    44,    44,   739,   416,    38,
      39,   174,   672,   111,   676,   136,   680,   681,   334,   336,
     529,   127,    40,   613,   613,   404,   139,   438,    51,   139,
      56,   128,    57,   128,   129,    40,   129,   140,   282,   283,
     442,   346,   693,   694,   695,   696,   424,     2,   572,   114,
       4,   525,     5,     6,     2,     8,     9,     4,   608,     5,
       6,   634,     8,     9,    40,    61,    62,    63,   167,    65,
      49,   433,   655,   525,   137,   614,   614,   434,   655,   366,
     367,   368,   369,   138,   372,   373,   374,   375,   526,   365,
     525,   380,   381,   382,   383,   384,   385,   386,   387,   388,
     389,   260,   394,   261,    66,   582,   119,    67,    68,    69,
      70,    71,    72,    73,    74,   456,    75,    76,   653,   411,
     412,    50,   118,   524,   656,   660,   128,   285,   286,   129,
     656,   485,    77,    78,    79,   121,   615,   615,    80,    81,
     616,   616,   263,   146,   537,    82,   522,   264,   147,   168,
    -164,   148,    84,    40,   149,   525,   169,   617,   617,   119,
      36,   151,    37,   557,   152,   636,   446,   511,   512,   513,
     514,     1,     2,   120,     3,     4,  -124,     5,     6,     7,
       8,     9,    10,   376,    11,   657,   377,   469,   309,   658,
     310,   657,   122,   473,   128,   658,   127,   129,   476,   477,
     478,   479,   480,   481,   482,   483,   659,   350,   128,   573,
     128,   129,   659,   129,   448,    38,    39,   449,   312,    13,
     313,   487,   488,   574,   158,   575,   467,   468,   133,   579,
     579,   244,   245,   257,   258,   259,   489,   351,   492,   493,
     590,    40,   457,   135,   146,   127,   146,   363,   364,   147,
     274,   147,   148,   141,   148,   149,   509,   149,   246,   247,
     248,   314,   142,    57,   252,   253,   254,   255,   256,   257,
     258,   259,   153,    44,    44,    44,    44,   453,   454,    61,
      62,    63,    64,    65,    49,   254,   255,   256,   257,   258,
     259,   154,   539,   541,    55,    55,   627,  -165,  -165,  -166,
    -166,  -165,   545,  -166,   546,   547,   548,   549,   550,   551,
     552,     2,   470,   155,     4,   449,     5,     6,    66,     8,
       9,    67,    68,    69,    70,    71,    72,    73,    74,   471,
      75,    76,   449,   562,   472,    50,   156,   449,   128,   559,
     494,   129,   316,   449,   317,   570,    77,    78,    79,   411,
     157,   595,    80,    81,   449,   128,   258,   259,   129,    82,
     279,   276,   353,    83,   162,   370,    84,   371,   277,   417,
     561,   418,   698,   163,   699,   652,   252,   253,   254,   255,
     256,   257,   258,   259,    61,    62,    63,    64,    65,    49,
     701,   704,   702,   705,   581,   364,   164,   606,   607,   578,
     580,    40,   294,   290,   300,   311,   318,   320,   322,   620,
     332,   337,   323,   338,   339,   340,   341,   355,   395,   396,
     356,   378,   357,    66,   358,   359,    67,    68,    69,    70,
      71,    72,    73,    74,   360,    75,    76,   361,   362,   379,
      50,   398,   401,   643,   644,   645,   402,   647,   403,   407,
     413,    77,    78,    79,   419,   420,   712,    80,    81,   324,
     421,   422,   430,   432,    82,   435,   443,   444,    83,   316,
     325,    84,   439,   440,   317,    61,    62,    63,    64,    65,
      49,   441,   445,   447,   450,   455,   486,   458,   500,   464,
     461,   689,     1,     2,   462,     3,     4,  -124,     5,     6,
       7,     8,     9,    10,   463,    11,   691,   465,   474,   499,
     501,   502,   506,  -178,    66,  -179,   515,    67,    68,    69,
      70,    71,    72,    73,    74,   516,    75,    76,   158,   519,
     710,    50,  -180,   523,   530,   531,   532,   533,   543,    12,
      13,   544,    77,    78,    79,   565,   568,   593,    80,    81,
     569,   596,   594,   597,   598,    82,   727,   728,   609,    83,
     619,   495,    84,   622,   623,   629,   726,   630,   737,    61,
      62,    63,    64,    65,    49,   631,   525,   639,   393,   640,
     662,   747,   748,   749,   641,   642,   646,   661,   667,   668,
     -27,     1,     2,   753,     3,     4,  -124,     5,     6,     7,
       8,     9,    10,   682,    11,   683,   687,   690,    66,   692,
     697,    67,    68,    69,    70,    71,    72,    73,    74,   700,
      75,    76,   708,   709,   715,    50,    61,    62,    63,    64,
      65,    49,   703,   706,   720,   721,    77,    78,    79,    13,
     707,   110,    80,    81,   716,   717,   718,   722,   723,    82,
     719,   733,   621,    83,   724,   618,    84,    47,   303,   729,
     730,   731,   732,   734,   735,    66,   736,   742,    67,    68,
      69,    70,    71,    72,    73,    74,   743,    75,    76,   744,
     745,   571,    50,    61,    62,    63,    64,    65,    49,   504,
     415,   315,   319,    77,    78,    79,   293,   626,   542,    80,
      81,   343,   321,   518,   484,     0,    82,     0,     0,     0,
      83,     0,   399,    84,     0,     0,     0,     0,     0,     0,
       0,     0,    66,     0,     0,    67,    68,    69,    70,    71,
      72,    73,    74,     0,    75,    76,     0,     0,     0,    50,
      61,    62,    63,    64,    65,    49,     0,     0,     0,     0,
      77,    78,    79,     0,     0,     0,    80,    81,     0,     0,
       0,     0,     0,    82,     0,     0,     0,    83,     0,   536,
      84,     0,     0,     0,     0,     0,     0,     0,     0,    66,
       0,     0,    67,    68,    69,    70,    71,    72,    73,    74,
       0,    75,    76,     0,     0,     0,    50,    61,    62,    63,
      64,    65,    49,     0,     0,     0,     0,    77,    78,    79,
       0,     0,     0,    80,    81,     0,     0,     0,     0,   540,
      82,     0,     0,     0,    83,     0,     0,    84,     0,     0,
       0,     0,     0,     0,     0,     0,    66,     0,     0,    67,
      68,    69,    70,    71,    72,    73,    74,     0,    75,    76,
       0,     0,     0,    50,    61,    62,    63,   171,    65,   172,
       0,     0,     0,     0,    77,    78,    79,     0,     0,     0,
      80,    81,     0,     0,     0,     0,     0,    82,     0,     0,
       0,    83,     0,     0,    84,     0,     0,     0,     0,     0,
       0,     0,     0,    66,     0,     0,    67,    68,    69,    70,
      71,    72,    73,    74,     0,    75,    76,     0,     0,     0,
     173,    61,    62,    63,   171,    65,   172,     0,     0,     0,
       0,    77,    78,    79,     0,     0,     0,    80,    81,     0,
       0,     0,     0,     0,    82,     0,     0,     0,    83,     0,
       0,    84,     0,     0,     0,     0,     0,     0,     0,     0,
      66,     0,     0,    67,    68,    69,    70,    71,    72,    73,
      74,     0,    75,    76,     0,     0,     0,   173,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,    77,    78,
      79,     0,     0,     0,    80,    81,     0,     0,     0,   175,
     176,    82,   177,     0,     0,    83,     0,     0,    84,     0,
     178,   179,   180,   181,   182,   183,   184,   185,   186,   187,
     188,   189,   190,   191,   192,   193,   194,   195,   196,   197,
     198,   199,   200,   201,   202,   203,   204,   205,   206,   207,
     208,   209,   210,   211,   212,   213,   214,   215,   216,   217,
     218,   219,   220,   221,   222,     0,     0,   223,     0,   224,
     225,   226,   227,     0,   228,   229,   230,     0,     0,     0,
       0,     0,     0,     0,     0,   175,   176,     0,   177,     0,
       0,     0,     0,     0,     0,   231,   178,   179,   180,   181,
     182,   183,   184,   185,   186,   187,   188,   189,   190,   191,
     192,   193,   194,   195,   196,   197,   198,   199,   200,   201,
     202,   203,   204,   205,   206,   207,   208,   209,   210,   211,
     212,   213,   214,   215,   216,   217,   218,   219,   220,   221,
     222,     0,     0,   223,     0,   224,   225,   226,   227,     0,
     228,   229,   230,   236,   237,   238,     0,     0,     0,   239,
       0,     0,     0,   554,     0,     0,     0,     0,     0,     0,
       0,   517,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,   241,   242,   243,     0,   244,   245,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,   236,
     237,   238,   490,     0,     0,   239,     0,     0,     0,     0,
       0,     0,     0,     0,   246,   247,   248,   249,   250,   251,
     252,   253,   254,   255,   256,   257,   258,   259,     0,   241,
     242,   243,   555,   244,   245,     0,     0,     0,   556,     0,
       0,     0,     0,     0,   236,   237,   238,     0,     0,     0,
     239,     0,     0,     0,     0,     0,     0,     0,     0,     0,
     246,   247,   248,   249,   250,   251,   252,   253,   254,   255,
     256,   257,   258,   259,   241,   242,   243,     0,   244,   245,
       0,     0,     0,     0,   491,     0,     0,     0,     0,     0,
     236,   237,   238,     0,     0,     0,   239,     0,     0,     0,
       0,     0,     0,     0,     0,   246,   247,   248,   249,   250,
     251,   252,   253,   254,   255,   256,   257,   258,   259,     0,
     241,   242,   243,   648,   244,   245,     0,     0,     0,   649,
       0,     0,     0,     0,     0,   236,   237,   238,     0,     0,
       0,   239,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   246,   247,   248,   249,   250,   251,   252,   253,   254,
     255,   256,   257,   258,   259,   241,   242,   243,     0,   244,
     245,     0,     0,     0,     0,   560,     0,     0,     0,     0,
     236,   237,   238,     0,     0,     0,   239,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   246,   247,   248,   249,
     250,   251,   252,   253,   254,   255,   256,   257,   258,   259,
     241,   242,   243,     0,   244,   245,     0,     0,     0,     0,
     610,     0,     0,     0,     0,   236,   237,   238,     0,     0,
       0,   239,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   246,   247,   248,   249,   250,   251,   252,   253,   254,
     255,   256,   257,   258,   259,   241,   242,   243,     0,   244,
     245,     0,     0,     0,     0,   650,     0,     0,     0,     0,
     236,   237,   238,     0,     0,     0,   239,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   246,   247,   248,   249,
     250,   251,   252,   253,   254,   255,   256,   257,   258,   259,
     241,   242,   243,     0,   244,   245,     0,     0,     0,     0,
     711,     0,     0,     0,     0,   236,   237,   238,     0,     0,
       0,   239,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   246,   247,   248,   249,   250,   251,   252,   253,   254,
     255,   256,   257,   258,   259,   241,   242,   243,     0,   244,
     245,     0,     0,     0,     0,   741,     0,     0,     0,     0,
     236,   237,   238,     0,     0,     0,   239,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   246,   247,   248,   249,
     250,   251,   252,   253,   254,   255,   256,   257,   258,   259,
     241,   242,   243,     0,   244,   245,     0,     0,     0,     0,
     751,     0,     0,     0,     0,   236,   237,   238,     0,     0,
       0,   239,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   246,   247,   248,   249,   250,   251,   252,   253,   254,
     255,   256,   257,   258,   259,   241,   242,   243,     0,   244,
     245,     0,     0,     0,     0,   752,     0,     0,     0,     0,
     236,   237,   238,     0,     0,     0,   239,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   246,   247,   248,   249,
     250,   251,   252,   253,   254,   255,   256,   257,   258,   259,
     241,   242,   243,     0,   244,   245,     0,     0,     0,     0,
     754,     0,     0,     0,     0,   236,   237,   238,     0,     0,
       0,   239,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   246,   247,   248,   249,   250,   251,   252,   253,   254,
     255,   256,   257,   258,   259,   241,   242,   243,     0,   244,
     245,     0,     0,   603,     0,     0,     0,     0,     0,     0,
     236,   237,   238,     0,     0,     0,   239,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   246,   247,   248,   249,
     250,   251,   252,   253,   254,   255,   256,   257,   258,   259,
     241,   242,   243,     0,   244,   245,     0,   599,     0,     0,
       0,     0,     0,     0,     0,     0,     0,   236,   237,   238,
       0,     0,     0,   239,     0,     0,     0,     0,     0,     0,
       0,   246,   247,   248,   249,   250,   251,   252,   253,   254,
     255,   256,   257,   258,   259,     0,     0,   241,   242,   243,
     301,   244,   245,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,   236,   237,   238,     0,     0,     0,
     239,     0,     0,     0,     0,     0,     0,     0,   246,   247,
     248,   249,   250,   251,   252,   253,   254,   255,   256,   257,
     258,   259,     0,     0,   241,   242,   243,   354,   244,   245,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   236,   237,   238,     0,     0,     0,   239,     0,     0,
       0,     0,     0,     0,     0,   246,   247,   248,   249,   250,
     251,   252,   253,   254,   255,   256,   257,   258,   259,     0,
       0,   241,   242,   243,   538,   244,   245,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,   236,   237,
     238,     0,     0,     0,   239,     0,     0,     0,     0,     0,
       0,     0,   246,   247,   248,   249,   250,   251,   252,   253,
     254,   255,   256,   257,   258,   259,     0,     0,   241,   242,
     243,   684,   244,   245,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,   236,   237,   238,     0,     0,
       0,   239,     0,     0,     0,     0,     0,     0,     0,   246,
     247,   248,   249,   250,   251,   252,   253,   254,   255,   256,
     257,   258,   259,     0,     0,   241,   242,   243,   685,   244,
     245,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,   236,   237,   238,     0,     0,     0,   239,     0,
       0,     0,     0,     0,     0,     0,   246,   247,   248,   249,
     250,   251,   252,   253,   254,   255,   256,   257,   258,   259,
       0,     0,   241,   242,   243,   686,   244,   245,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,   236,
     237,   238,     0,     0,     0,   239,     0,     0,     0,     0,
       0,     0,     0,   246,   247,   248,   249,   250,   251,   252,
     253,   254,   255,   256,   257,   258,   259,     0,     0,   241,
     242,   243,   688,   244,   245,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   236,   237,   238,     0,
       0,     0,   239,     0,     0,     0,     0,     0,     0,     0,
     246,   247,   248,   249,   250,   251,   252,   253,   254,   255,
     256,   257,   258,   259,     0,     0,   241,   242,   243,   746,
     244,   245,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,   236,   237,   238,     0,     0,     0,   239,     0,
       0,     0,     0,     0,     0,     0,     0,   246,   247,   248,
     249,   250,   251,   252,   253,   254,   255,   256,   257,   258,
     259,     0,   241,   242,   243,   563,   244,   245,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,   236,   237,
     238,     0,     0,     0,   239,     0,     0,     0,     0,     0,
       0,     0,     0,   246,   247,   248,   249,   250,   251,   252,
     253,   254,   255,   256,   257,   258,   259,     0,   241,   242,
     243,   564,   244,   245,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,   236,   237,   238,     0,     0,     0,
     239,     0,     0,     0,     0,     0,     0,     0,     0,   246,
     247,   248,   249,   250,   251,   252,   253,   254,   255,   256,
     257,   258,   259,     0,   241,   242,   243,   725,   244,   245,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
     236,   237,   238,     0,     0,     0,   239,     0,     0,     0,
       0,     0,     0,     0,     0,   246,   247,   248,   249,   250,
     251,   252,   253,   254,   255,   256,   257,   258,   259,     0,
     241,   242,   243,   740,   244,   245,     0,     1,     2,     0,
       3,     4,  -124,     5,     6,     7,     8,     9,    10,     0,
      11,     0,   236,   237,   238,     0,     0,     0,   239,     0,
       0,   246,   247,   248,   249,   250,   251,   252,   253,   254,
     255,   256,   257,   258,   259,     0,     0,     0,     0,   750,
     507,   508,   241,   242,   243,    13,   244,   245,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
     236,   237,   238,     0,     0,     0,   239,     0,     0,     0,
       0,     0,     0,   246,   247,   248,   249,   250,   251,   252,
     253,   254,   255,   256,   257,   258,   259,   240,     0,     0,
     241,   242,   243,     0,   244,   245,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,   236,   237,
     238,     0,     0,     0,   239,     0,     0,     0,     0,     0,
       0,   246,   247,   248,   249,   250,   251,   252,   253,   254,
     255,   256,   257,   258,   259,   281,     0,     0,   241,   242,
     243,     0,   244,   245,     0,     0,     0,     0,     0,     0,
       0,     0,     0,   236,   237,   238,     0,     0,     0,   239,
       0,     0,     0,     0,     0,     0,     0,     0,     0,   246,
     247,   248,   249,   250,   251,   252,   253,   254,   255,   256,
     257,   258,   259,   241,   242,   243,     0,   244,   245,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
     158,   236,   237,   238,     0,     0,     0,   239,     0,     0,
       0,     0,     0,     0,   246,   247,   248,   249,   250,   251,
     252,   253,   254,   255,   256,   257,   258,   259,     0,     0,
       0,   241,   242,   243,     0,   244,   245,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   236,   237,   238,   466,
       0,     0,   239,     0,     0,     0,     0,     0,     0,     0,
       0,     0,   246,   247,   248,   249,   250,   251,   252,   253,
     254,   255,   256,   257,   258,   259,   241,   242,   243,     0,
     244,   245,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   236,   237,   238,     0,     0,   553,   239,     0,     0,
       0,   600,     0,     0,     0,     0,     0,   246,   247,   248,
     249,   250,   251,   252,   253,   254,   255,   256,   257,   258,
     259,   241,   242,   243,     0,   244,   245,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   236,   237,   238,     0,
       0,     0,   239,     0,     0,     0,   601,     0,     0,     0,
       0,     0,   246,   247,   248,   249,   250,   251,   252,   253,
     254,   255,   256,   257,   258,   259,   241,   242,   243,     0,
     244,   245,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   236,   237,   238,     0,     0,     0,   239,     0,     0,
       0,   602,     0,     0,     0,     0,     0,   246,   247,   248,
     249,   250,   251,   252,   253,   254,   255,   256,   257,   258,
     259,   241,   242,   243,     0,   244,   245,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   236,   237,   238,     0,
       0,     0,   239,     0,     0,     0,   604,     0,     0,     0,
       0,     0,   246,   247,   248,   249,   250,   251,   252,   253,
     254,   255,   256,   257,   258,   259,   241,   242,   243,     0,
     244,   245,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   236,   237,   238,     0,     0,     0,   239,     0,     0,
       0,     0,     0,     0,     0,     0,     0,   246,   247,   248,
     249,   250,   251,   252,   253,   254,   255,   256,   257,   258,
     259,   241,   242,   243,     0,   244,   245,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   236,   237,   238,     0,
       0,   605,   239,     0,     0,     0,     0,     0,     0,     0,
       0,     0,   246,   247,   248,   249,   250,   251,   252,   253,
     254,   255,   256,   257,   258,   259,   241,   242,   243,     0,
     244,   245,     0,     0,     0,     0,     0,     0,     0,     0,
       0,   236,   237,   238,     0,     0,     0,   239,     0,     0,
       0,     0,     0,     0,     0,     0,     0,   246,   247,   248,
     249,   250,   251,   252,   253,   254,   255,   256,   257,   258,
     259,   241,     0,   243,     0,   244,   245,     0,     0,     0,
       0,     0,     0,     0,     0,     0,   236,   237,   238,     0,
       0,     0,   239,     0,     0,     0,     0,     0,     0,     0,
       0,     0,   246,   247,   248,   249,   250,   251,   252,   253,
     254,   255,   256,   257,   258,   259,   241,     0,     0,     0,
     244,   245,     0,     0,     0,     0,     0,   236,   237,   238,
       0,     1,     2,   239,     3,     4,  -124,     5,     6,     7,
       8,     9,    10,     0,    11,     0,     0,   246,   247,   248,
     249,   250,   251,   252,   253,   254,   255,   256,   257,   258,
     259,   244,   245,     0,     0,     0,     0,     0,   269,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,    13,
       0,     0,     0,     0,     0,     0,     0,     0,   246,   247,
     248,   249,   250,   251,   252,   253,   254,   255,   256,   257,
     258,   259,   175,   176,     0,   177,     0,     0,     0,     0,
       0,     0,     0,   178,   179,   180,   181,   182,   183,   184,
     185,   186,   187,   188,   189,   190,   191,   192,   193,   194,
     195,   196,   197,   198,   199,   200,   201,   202,   203,   204,
     205,   206,   207,   208,   209,   210,   211,   212,   213,   214,
     215,   216,   217,   218,   219,   220,   221,   222,     0,     0,
     223,     0,   224,   225,   226,   227,     0,   228,   229,   230,
     306,     0,   307,     0,     0,     0,     0,     0,     0,     0,
     178,   179,   180,   181,   182,   183,   184,   185,   186,   187,
     188,   189,   190,   191,   192,   193,   194,   195,   196,   197,
     198,     0,     0,   201,   202,   203,   204,   205,   206,   207,
       0,     0,   210,   211,   212,   213,   214,   215,   216,   217,
     218,   219,   220,   221,   222,     0,     0,   223,     0,   224,
     225,   226,   227,     0,   228,   229,   230,   390,     0,   391,
       0,     0,     0,     0,     0,     0,     0,   178,   179,   180,
     181,   182,   183,   184,   185,   186,   187,   188,   189,   190,
     191,   192,   193,   194,   195,   196,   197,   198,     0,     0,
     201,   202,   203,   204,   205,   206,   207,     0,     0,   210,
     211,   212,   213,   214,   215,   216,   217,   218,   219,   220,
     221,   222,     0,     0,   223,     0,   224,   225,   226,   227,
       0,   228,   229,   230,   425,     0,   426,     0,     0,     0,
       0,     0,     0,     0,   178,   179,   180,   181,   182,   183,
     184,   185,   186,   187,   188,   189,   190,   191,   192,   193,
     194,   195,   196,   197,   198,     0,     0,   201,   202,   203,
     204,   205,   206,   207,     0,     0,   210,   211,   212,   213,
     214,   215,   216,   217,   218,   219,   220,   221,   222,     0,
       0,   223,     0,   224,   225,   226,   227,     0,   228,   229,
     230,   520,     0,   521,     0,     0,     0,     0,     0,     0,
       0,   178,   179,   180,   181,   182,   183,   184,   185,   186,
     187,   188,   189,   190,   191,   192,   193,   194,   195,   196,
     197,   198,     0,     0,   201,   202,   203,   204,   205,   206,
     207,     0,     0,   210,   211,   212,   213,   214,   215,   216,
     217,   218,   219,   220,   221,   222,     0,     0,   223,     0,
     224,   225,   226,   227,     0,   228,   229,   230,   576,     0,
     577,     0,     0,     0,     0,     0,     0,     0,   178,   179,
     180,   181,   182,   183,   184,   185,   186,   187,   188,   189,
     190,   191,   192,   193,   194,   195,   196,   197,   198,     0,
       0,   201,   202,   203,   204,   205,   206,   207,     0,     0,
     210,   211,   212,   213,   214,   215,   216,   217,   218,   219,
     220,   221,   222,     0,     0,   223,     0,   224,   225,   226,
     227,     0,   228,   229,   230,   587,     0,   588,     0,     0,
       0,     0,     0,     0,     0,   178,   179,   180,   181,   182,
     183,   184,   185,   186,   187,   188,   189,   190,   191,   192,
     193,   194,   195,   196,   197,   198,     0,     0,   201,   202,
     203,   204,   205,   206,   207,     0,     0,   210,   211,   212,
     213,   214,   215,   216,   217,   218,   219,   220,   221,   222,
       0,     0,   223,     0,   224,   225,   226,   227,     0,   228,
     229,   230,   624,     0,   625,     0,     0,     0,     0,     0,
       0,     0,   178,   179,   180,   181,   182,   183,   184,   185,
     186,   187,   188,   189,   190,   191,   192,   193,   194,   195,
     196,   197,   198,     0,     0,   201,   202,   203,   204,   205,
     206,   207,     0,     0,   210,   211,   212,   213,   214,   215,
     216,   217,   218,   219,   220,   221,   222,     0,     0,   223,
       0,   224,   225,   226,   227,     0,   228,   229,   230
};

static const yytype_int16 yycheck[] =
{
       7,    85,   138,   352,   475,     4,     2,   168,   160,   131,
       6,   162,   163,   164,   108,   437,   524,   119,   120,   121,
     122,    19,     6,     6,     8,    46,     6,     6,    35,     8,
     298,     6,     6,     6,    53,     8,    12,    67,    32,    31,
      47,   563,   564,    87,    51,    53,    74,    78,     6,    77,
       8,    53,    66,    31,    47,    20,   527,    88,   152,    66,
      19,    89,    92,   443,   444,    77,     6,    86,     6,    76,
       8,    83,    19,    80,    81,    82,    83,     0,     3,   587,
     588,   589,   590,    91,    86,    22,    78,    12,    19,    82,
     612,     6,   113,     8,    92,   116,   618,     6,    92,     8,
      78,    59,    67,    78,     6,   112,     8,   529,    82,    93,
      90,   132,   583,   264,    93,   111,   624,   625,   626,   627,
      93,    59,   258,   119,   120,   121,   122,   103,   111,   136,
     137,   127,    91,    92,   130,   296,   135,   608,    78,    60,
     408,    88,    56,    78,    58,    59,    61,    62,   416,    87,
      59,   158,    77,   275,   290,   151,   152,    88,   280,    61,
      62,   168,   633,    16,   635,    14,   637,   638,   151,   152,
     438,    92,    87,   563,   564,   269,    82,   299,    87,    82,
       6,    74,     8,    74,    77,    87,    77,    93,    61,    62,
      93,   343,   663,   664,   665,   666,    89,    18,    89,    32,
      21,    78,    23,    24,    18,    26,    27,    21,   557,    23,
      24,    88,    26,    27,    87,     3,     4,     5,     6,     7,
       8,    77,   612,    78,    73,   563,   564,    83,   618,   236,
     237,   238,   239,    82,   241,   242,   243,   244,    93,   235,
      78,   248,   249,   250,   251,   252,   253,   254,   255,   256,
     257,     6,   259,     8,    42,    93,    73,    45,    46,    47,
      48,    49,    50,    51,    52,    82,    54,    55,    89,   276,
     277,    59,   279,   434,   612,    89,    74,    61,    62,    77,
     618,   365,    70,    71,    72,    87,   563,   564,    76,    77,
     563,   564,    82,    82,   445,    83,   432,    87,    87,    87,
      89,    90,    90,    87,    93,    78,    94,   563,   564,    73,
       6,    87,     8,   474,    90,    88,   323,   419,   420,   421,
     422,    17,    18,    73,    20,    21,    22,    23,    24,    25,
      26,    27,    28,    42,    30,   612,    45,   344,     6,   612,
       8,   618,    87,   350,    74,   618,    92,    77,   355,   356,
     357,   358,   359,   360,   361,   362,   612,    43,    74,    89,
      74,    77,   618,    77,    89,    61,    62,    92,     6,    65,
       8,   378,   379,    89,    56,    89,    58,    59,    93,   515,
     516,    43,    44,    81,    82,    83,   393,    73,   395,   396,
     526,    87,    89,    92,    82,    92,    82,    91,    92,    87,
     407,    87,    90,    82,    90,    93,   413,    93,    70,    71,
      72,     6,    92,     8,    76,    77,    78,    79,    80,    81,
      82,    83,    87,   419,   420,   421,   422,    91,    92,     3,
       4,     5,     6,     7,     8,    78,    79,    80,    81,    82,
      83,    87,   449,   450,   443,   444,   582,    89,    90,    89,
      90,    93,   459,    93,   461,   462,   463,   464,   465,   466,
     467,    18,    89,    87,    21,    92,    23,    24,    42,    26,
      27,    45,    46,    47,    48,    49,    50,    51,    52,    89,
      54,    55,    92,   490,    89,    59,    87,    92,    74,   485,
      89,    77,    16,    92,    18,   502,    70,    71,    72,   506,
      87,    89,    76,    77,    92,    74,    82,    83,    77,    83,
      92,    33,    94,    87,    87,     6,    90,     8,    34,     6,
      94,     8,     6,    87,     8,   609,    76,    77,    78,    79,
      80,    81,    82,    83,     3,     4,     5,     6,     7,     8,
       6,     6,     8,     8,    91,    92,    87,   554,   555,   515,
     516,    87,    77,    93,     6,     6,    60,    60,     6,   566,
       6,     6,    31,     6,     6,     6,     6,    93,    43,    43,
      93,    20,    93,    42,    93,    93,    45,    46,    47,    48,
      49,    50,    51,    52,    93,    54,    55,    93,    93,    20,
      59,     6,    29,   600,   601,   602,    68,   604,     5,    92,
      35,    70,    71,    72,    87,    87,   690,    76,    77,    78,
      87,    87,    90,    93,    83,    75,    21,    21,    87,    16,
      89,    90,    93,    93,    18,     3,     4,     5,     6,     7,
       8,    93,    87,    89,    93,    93,    45,    91,    36,    73,
      43,   648,    17,    18,    43,    20,    21,    22,    23,    24,
      25,    26,    27,    28,    43,    30,   652,    43,    87,    67,
       6,    19,    92,    88,    42,    88,    88,    45,    46,    47,
      48,    49,    50,    51,    52,    88,    54,    55,    56,    89,
     687,    59,    88,    75,     6,     6,     6,     6,     6,    64,
      65,     6,    70,    71,    72,    36,     5,    93,    76,    77,
       6,     6,    93,    93,    89,    83,   713,   714,    89,    87,
       6,     6,    90,    36,    69,    12,   712,     3,   725,     3,
       4,     5,     6,     7,     8,     3,    78,    94,    12,     6,
       5,   738,   739,   740,     6,     6,     6,     6,    94,     3,
      16,    17,    18,   750,    20,    21,    22,    23,    24,    25,
      26,    27,    28,    77,    30,    93,    43,    89,    42,    69,
      77,    45,    46,    47,    48,    49,    50,    51,    52,    94,
      54,    55,    75,     6,     5,    59,     3,     4,     5,     6,
       7,     8,    94,    94,    75,    77,    70,    71,    72,    65,
      94,    17,    76,    77,    94,    94,    94,    77,    77,    83,
      94,    75,   567,    87,    77,   564,    90,     3,   135,    77,
      77,    77,    77,    75,    75,    42,    75,    75,    45,    46,
      47,    48,    49,    50,    51,    52,    75,    54,    55,    75,
      75,   506,    59,     3,     4,     5,     6,     7,     8,   407,
     279,   142,   144,    70,    71,    72,   127,   582,   454,    76,
      77,   159,   145,   430,   364,    -1,    83,    -1,    -1,    -1,
      87,    -1,    89,    90,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    42,    -1,    -1,    45,    46,    47,    48,    49,
      50,    51,    52,    -1,    54,    55,    -1,    -1,    -1,    59,
       3,     4,     5,     6,     7,     8,    -1,    -1,    -1,    -1,
      70,    71,    72,    -1,    -1,    -1,    76,    77,    -1,    -1,
      -1,    -1,    -1,    83,    -1,    -1,    -1,    87,    -1,    89,
      90,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    42,
      -1,    -1,    45,    46,    47,    48,    49,    50,    51,    52,
      -1,    54,    55,    -1,    -1,    -1,    59,     3,     4,     5,
       6,     7,     8,    -1,    -1,    -1,    -1,    70,    71,    72,
      -1,    -1,    -1,    76,    77,    -1,    -1,    -1,    -1,    82,
      83,    -1,    -1,    -1,    87,    -1,    -1,    90,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    42,    -1,    -1,    45,
      46,    47,    48,    49,    50,    51,    52,    -1,    54,    55,
      -1,    -1,    -1,    59,     3,     4,     5,     6,     7,     8,
      -1,    -1,    -1,    -1,    70,    71,    72,    -1,    -1,    -1,
      76,    77,    -1,    -1,    -1,    -1,    -1,    83,    -1,    -1,
      -1,    87,    -1,    -1,    90,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    42,    -1,    -1,    45,    46,    47,    48,
      49,    50,    51,    52,    -1,    54,    55,    -1,    -1,    -1,
      59,     3,     4,     5,     6,     7,     8,    -1,    -1,    -1,
      -1,    70,    71,    72,    -1,    -1,    -1,    76,    77,    -1,
      -1,    -1,    -1,    -1,    83,    -1,    -1,    -1,    87,    -1,
      -1,    90,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      42,    -1,    -1,    45,    46,    47,    48,    49,    50,    51,
      52,    -1,    54,    55,    -1,    -1,    -1,    59,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    70,    71,
      72,    -1,    -1,    -1,    76,    77,    -1,    -1,    -1,     5,
       6,    83,     8,    -1,    -1,    87,    -1,    -1,    90,    -1,
      16,    17,    18,    19,    20,    21,    22,    23,    24,    25,
      26,    27,    28,    29,    30,    31,    32,    33,    34,    35,
      36,    37,    38,    39,    40,    41,    42,    43,    44,    45,
      46,    47,    48,    49,    50,    51,    52,    53,    54,    55,
      56,    57,    58,    59,    60,    -1,    -1,    63,    -1,    65,
      66,    67,    68,    -1,    70,    71,    72,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,     5,     6,    -1,     8,    -1,
      -1,    -1,    -1,    -1,    -1,    91,    16,    17,    18,    19,
      20,    21,    22,    23,    24,    25,    26,    27,    28,    29,
      30,    31,    32,    33,    34,    35,    36,    37,    38,    39,
      40,    41,    42,    43,    44,    45,    46,    47,    48,    49,
      50,    51,    52,    53,    54,    55,    56,    57,    58,    59,
      60,    -1,    -1,    63,    -1,    65,    66,    67,    68,    -1,
      70,    71,    72,     9,    10,    11,    -1,    -1,    -1,    15,
      -1,    -1,    -1,    19,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    91,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    39,    40,    41,    -1,    43,    44,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,     9,
      10,    11,    12,    -1,    -1,    15,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    70,    71,    72,    73,    74,    75,
      76,    77,    78,    79,    80,    81,    82,    83,    -1,    39,
      40,    41,    88,    43,    44,    -1,    -1,    -1,    94,    -1,
      -1,    -1,    -1,    -1,     9,    10,    11,    -1,    -1,    -1,
      15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      70,    71,    72,    73,    74,    75,    76,    77,    78,    79,
      80,    81,    82,    83,    39,    40,    41,    -1,    43,    44,
      -1,    -1,    -1,    -1,    94,    -1,    -1,    -1,    -1,    -1,
       9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    70,    71,    72,    73,    74,
      75,    76,    77,    78,    79,    80,    81,    82,    83,    -1,
      39,    40,    41,    88,    43,    44,    -1,    -1,    -1,    94,
      -1,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,    -1,
      -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    70,    71,    72,    73,    74,    75,    76,    77,    78,
      79,    80,    81,    82,    83,    39,    40,    41,    -1,    43,
      44,    -1,    -1,    -1,    -1,    94,    -1,    -1,    -1,    -1,
       9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,    73,
      74,    75,    76,    77,    78,    79,    80,    81,    82,    83,
      39,    40,    41,    -1,    43,    44,    -1,    -1,    -1,    -1,
      94,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,    -1,
      -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    70,    71,    72,    73,    74,    75,    76,    77,    78,
      79,    80,    81,    82,    83,    39,    40,    41,    -1,    43,
      44,    -1,    -1,    -1,    -1,    94,    -1,    -1,    -1,    -1,
       9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,    73,
      74,    75,    76,    77,    78,    79,    80,    81,    82,    83,
      39,    40,    41,    -1,    43,    44,    -1,    -1,    -1,    -1,
      94,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,    -1,
      -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    70,    71,    72,    73,    74,    75,    76,    77,    78,
      79,    80,    81,    82,    83,    39,    40,    41,    -1,    43,
      44,    -1,    -1,    -1,    -1,    94,    -1,    -1,    -1,    -1,
       9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,    73,
      74,    75,    76,    77,    78,    79,    80,    81,    82,    83,
      39,    40,    41,    -1,    43,    44,    -1,    -1,    -1,    -1,
      94,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,    -1,
      -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    70,    71,    72,    73,    74,    75,    76,    77,    78,
      79,    80,    81,    82,    83,    39,    40,    41,    -1,    43,
      44,    -1,    -1,    -1,    -1,    94,    -1,    -1,    -1,    -1,
       9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,    73,
      74,    75,    76,    77,    78,    79,    80,    81,    82,    83,
      39,    40,    41,    -1,    43,    44,    -1,    -1,    -1,    -1,
      94,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,    -1,
      -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    70,    71,    72,    73,    74,    75,    76,    77,    78,
      79,    80,    81,    82,    83,    39,    40,    41,    -1,    43,
      44,    -1,    -1,    92,    -1,    -1,    -1,    -1,    -1,    -1,
       9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,    73,
      74,    75,    76,    77,    78,    79,    80,    81,    82,    83,
      39,    40,    41,    -1,    43,    44,    -1,    91,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,     9,    10,    11,
      -1,    -1,    -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    70,    71,    72,    73,    74,    75,    76,    77,    78,
      79,    80,    81,    82,    83,    -1,    -1,    39,    40,    41,
      89,    43,    44,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,     9,    10,    11,    -1,    -1,    -1,
      15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    70,    71,
      72,    73,    74,    75,    76,    77,    78,    79,    80,    81,
      82,    83,    -1,    -1,    39,    40,    41,    89,    43,    44,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,     9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    70,    71,    72,    73,    74,
      75,    76,    77,    78,    79,    80,    81,    82,    83,    -1,
      -1,    39,    40,    41,    89,    43,    44,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,     9,    10,
      11,    -1,    -1,    -1,    15,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    70,    71,    72,    73,    74,    75,    76,    77,
      78,    79,    80,    81,    82,    83,    -1,    -1,    39,    40,
      41,    89,    43,    44,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,    -1,
      -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    70,
      71,    72,    73,    74,    75,    76,    77,    78,    79,    80,
      81,    82,    83,    -1,    -1,    39,    40,    41,    89,    43,
      44,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,     9,    10,    11,    -1,    -1,    -1,    15,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,    73,
      74,    75,    76,    77,    78,    79,    80,    81,    82,    83,
      -1,    -1,    39,    40,    41,    89,    43,    44,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,     9,
      10,    11,    -1,    -1,    -1,    15,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    70,    71,    72,    73,    74,    75,    76,
      77,    78,    79,    80,    81,    82,    83,    -1,    -1,    39,
      40,    41,    89,    43,    44,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,
      -1,    -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      70,    71,    72,    73,    74,    75,    76,    77,    78,    79,
      80,    81,    82,    83,    -1,    -1,    39,    40,    41,    89,
      43,    44,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,     9,    10,    11,    -1,    -1,    -1,    15,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,
      73,    74,    75,    76,    77,    78,    79,    80,    81,    82,
      83,    -1,    39,    40,    41,    88,    43,    44,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,     9,    10,
      11,    -1,    -1,    -1,    15,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    70,    71,    72,    73,    74,    75,    76,
      77,    78,    79,    80,    81,    82,    83,    -1,    39,    40,
      41,    88,    43,    44,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,     9,    10,    11,    -1,    -1,    -1,
      15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    70,
      71,    72,    73,    74,    75,    76,    77,    78,    79,    80,
      81,    82,    83,    -1,    39,    40,    41,    88,    43,    44,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
       9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    70,    71,    72,    73,    74,
      75,    76,    77,    78,    79,    80,    81,    82,    83,    -1,
      39,    40,    41,    88,    43,    44,    -1,    17,    18,    -1,
      20,    21,    22,    23,    24,    25,    26,    27,    28,    -1,
      30,    -1,     9,    10,    11,    -1,    -1,    -1,    15,    -1,
      -1,    70,    71,    72,    73,    74,    75,    76,    77,    78,
      79,    80,    81,    82,    83,    -1,    -1,    -1,    -1,    88,
      37,    38,    39,    40,    41,    65,    43,    44,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
       9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,    -1,
      -1,    -1,    -1,    70,    71,    72,    73,    74,    75,    76,
      77,    78,    79,    80,    81,    82,    83,    36,    -1,    -1,
      39,    40,    41,    -1,    43,    44,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,     9,    10,
      11,    -1,    -1,    -1,    15,    -1,    -1,    -1,    -1,    -1,
      -1,    70,    71,    72,    73,    74,    75,    76,    77,    78,
      79,    80,    81,    82,    83,    36,    -1,    -1,    39,    40,
      41,    -1,    43,    44,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,     9,    10,    11,    -1,    -1,    -1,    15,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    70,
      71,    72,    73,    74,    75,    76,    77,    78,    79,    80,
      81,    82,    83,    39,    40,    41,    -1,    43,    44,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      56,     9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,
      -1,    -1,    -1,    -1,    70,    71,    72,    73,    74,    75,
      76,    77,    78,    79,    80,    81,    82,    83,    -1,    -1,
      -1,    39,    40,    41,    -1,    43,    44,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,     9,    10,    11,    57,
      -1,    -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    70,    71,    72,    73,    74,    75,    76,    77,
      78,    79,    80,    81,    82,    83,    39,    40,    41,    -1,
      43,    44,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,     9,    10,    11,    -1,    -1,    59,    15,    -1,    -1,
      -1,    19,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,
      73,    74,    75,    76,    77,    78,    79,    80,    81,    82,
      83,    39,    40,    41,    -1,    43,    44,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,
      -1,    -1,    15,    -1,    -1,    -1,    19,    -1,    -1,    -1,
      -1,    -1,    70,    71,    72,    73,    74,    75,    76,    77,
      78,    79,    80,    81,    82,    83,    39,    40,    41,    -1,
      43,    44,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,     9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,
      -1,    19,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,
      73,    74,    75,    76,    77,    78,    79,    80,    81,    82,
      83,    39,    40,    41,    -1,    43,    44,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,
      -1,    -1,    15,    -1,    -1,    -1,    19,    -1,    -1,    -1,
      -1,    -1,    70,    71,    72,    73,    74,    75,    76,    77,
      78,    79,    80,    81,    82,    83,    39,    40,    41,    -1,
      43,    44,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,     9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,
      73,    74,    75,    76,    77,    78,    79,    80,    81,    82,
      83,    39,    40,    41,    -1,    43,    44,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,
      -1,    59,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    70,    71,    72,    73,    74,    75,    76,    77,
      78,    79,    80,    81,    82,    83,    39,    40,    41,    -1,
      43,    44,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,     9,    10,    11,    -1,    -1,    -1,    15,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    70,    71,    72,
      73,    74,    75,    76,    77,    78,    79,    80,    81,    82,
      83,    39,    -1,    41,    -1,    43,    44,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,     9,    10,    11,    -1,
      -1,    -1,    15,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    70,    71,    72,    73,    74,    75,    76,    77,
      78,    79,    80,    81,    82,    83,    39,    -1,    -1,    -1,
      43,    44,    -1,    -1,    -1,    -1,    -1,     9,    10,    11,
      -1,    17,    18,    15,    20,    21,    22,    23,    24,    25,
      26,    27,    28,    -1,    30,    -1,    -1,    70,    71,    72,
      73,    74,    75,    76,    77,    78,    79,    80,    81,    82,
      83,    43,    44,    -1,    -1,    -1,    -1,    -1,    54,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    65,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    70,    71,
      72,    73,    74,    75,    76,    77,    78,    79,    80,    81,
      82,    83,     5,     6,    -1,     8,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    16,    17,    18,    19,    20,    21,    22,
      23,    24,    25,    26,    27,    28,    29,    30,    31,    32,
      33,    34,    35,    36,    37,    38,    39,    40,    41,    42,
      43,    44,    45,    46,    47,    48,    49,    50,    51,    52,
      53,    54,    55,    56,    57,    58,    59,    60,    -1,    -1,
      63,    -1,    65,    66,    67,    68,    -1,    70,    71,    72,
       6,    -1,     8,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      16,    17,    18,    19,    20,    21,    22,    23,    24,    25,
      26,    27,    28,    29,    30,    31,    32,    33,    34,    35,
      36,    -1,    -1,    39,    40,    41,    42,    43,    44,    45,
      -1,    -1,    48,    49,    50,    51,    52,    53,    54,    55,
      56,    57,    58,    59,    60,    -1,    -1,    63,    -1,    65,
      66,    67,    68,    -1,    70,    71,    72,     6,    -1,     8,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    16,    17,    18,
      19,    20,    21,    22,    23,    24,    25,    26,    27,    28,
      29,    30,    31,    32,    33,    34,    35,    36,    -1,    -1,
      39,    40,    41,    42,    43,    44,    45,    -1,    -1,    48,
      49,    50,    51,    52,    53,    54,    55,    56,    57,    58,
      59,    60,    -1,    -1,    63,    -1,    65,    66,    67,    68,
      -1,    70,    71,    72,     6,    -1,     8,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    16,    17,    18,    19,    20,    21,
      22,    23,    24,    25,    26,    27,    28,    29,    30,    31,
      32,    33,    34,    35,    36,    -1,    -1,    39,    40,    41,
      42,    43,    44,    45,    -1,    -1,    48,    49,    50,    51,
      52,    53,    54,    55,    56,    57,    58,    59,    60,    -1,
      -1,    63,    -1,    65,    66,    67,    68,    -1,    70,    71,
      72,     6,    -1,     8,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    16,    17,    18,    19,    20,    21,    22,    23,    24,
      25,    26,    27,    28,    29,    30,    31,    32,    33,    34,
      35,    36,    -1,    -1,    39,    40,    41,    42,    43,    44,
      45,    -1,    -1,    48,    49,    50,    51,    52,    53,    54,
      55,    56,    57,    58,    59,    60,    -1,    -1,    63,    -1,
      65,    66,    67,    68,    -1,    70,    71,    72,     6,    -1,
       8,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    16,    17,
      18,    19,    20,    21,    22,    23,    24,    25,    26,    27,
      28,    29,    30,    31,    32,    33,    34,    35,    36,    -1,
      -1,    39,    40,    41,    42,    43,    44,    45,    -1,    -1,
      48,    49,    50,    51,    52,    53,    54,    55,    56,    57,
      58,    59,    60,    -1,    -1,    63,    -1,    65,    66,    67,
      68,    -1,    70,    71,    72,     6,    -1,     8,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    16,    17,    18,    19,    20,
      21,    22,    23,    24,    25,    26,    27,    28,    29,    30,
      31,    32,    33,    34,    35,    36,    -1,    -1,    39,    40,
      41,    42,    43,    44,    45,    -1,    -1,    48,    49,    50,
      51,    52,    53,    54,    55,    56,    57,    58,    59,    60,
      -1,    -1,    63,    -1,    65,    66,    67,    68,    -1,    70,
      71,    72,     6,    -1,     8,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    16,    17,    18,    19,    20,    21,    22,    23,
      24,    25,    26,    27,    28,    29,    30,    31,    32,    33,
      34,    35,    36,    -1,    -1,    39,    40,    41,    42,    43,
      44,    45,    -1,    -1,    48,    49,    50,    51,    52,    53,
      54,    55,    56,    57,    58,    59,    60,    -1,    -1,    63,
      -1,    65,    66,    67,    68,    -1,    70,    71,    72
};

/* YYSTOS[STATE-NUM] -- The symbol kind of the accessing symbol of
   state STATE-NUM.  */
static const yytype_uint8 yystos[] =
{
       0,    17,    18,    20,    21,    23,    24,    25,    26,    27,
      28,    30,    64,    65,    96,    97,    98,    99,   100,   101,
     103,   104,   105,   106,   107,   108,   113,   127,   128,   131,
     132,   135,   138,    31,    78,   115,     6,     8,    61,    62,
      87,   139,   140,   141,   142,    31,    78,   115,     6,     8,
      59,    87,   124,   125,   126,   169,     6,     8,   136,   137,
     139,     3,     4,     5,     6,     7,    42,    45,    46,    47,
      48,    49,    50,    51,    52,    54,    55,    70,    71,    72,
      76,    77,    83,    87,    90,   142,   151,   152,   153,   154,
     155,   156,   158,   159,   160,   161,   162,   165,   168,   169,
     171,    87,     6,    90,   112,    97,    66,     0,    53,    86,
     100,    16,    22,    78,    32,   116,   122,   123,   151,    73,
      73,    87,    87,     6,     8,    59,   144,    92,    74,    77,
     143,   116,   122,    93,   151,    92,    14,    73,    82,    82,
      93,    82,    92,    60,   129,   130,    82,    87,    90,    93,
     151,    87,    90,    87,    87,    87,    87,    87,    56,   151,
     166,   167,    87,    87,    87,   151,   151,     6,    87,    94,
     122,     6,     8,    59,   151,     5,     6,     8,    16,    17,
      18,    19,    20,    21,    22,    23,    24,    25,    26,    27,
      28,    29,    30,    31,    32,    33,    34,    35,    36,    37,
      38,    39,    40,    41,    42,    43,    44,    45,    46,    47,
      48,    49,    50,    51,    52,    53,    54,    55,    56,    57,
      58,    59,    60,    63,    65,    66,    67,    68,    70,    71,
      72,    91,   170,   173,   174,   143,     9,    10,    11,    15,
      36,    39,    40,    41,    43,    44,    70,    71,    72,    73,
      74,    75,    76,    77,    78,    79,    80,    81,    82,    83,
       6,     8,    97,    82,    87,   109,    86,    20,    67,    54,
      98,   139,   133,   134,   151,   116,    33,    34,   117,    92,
     116,    36,    61,    62,   140,    61,    62,   140,   140,   140,
      93,   146,   149,   141,    77,    77,    83,   142,   117,   116,
       6,    89,     6,   126,   151,   151,     6,     8,   170,     6,
       8,     6,     6,     8,     6,   137,    16,    18,    60,   130,
      60,   129,     6,    31,    78,    89,   151,   157,     6,    82,
     163,   164,     6,     6,   139,    98,   139,     6,     6,     6,
       6,     6,   151,   166,    58,    59,   167,   157,   157,   157,
      43,    73,   144,    94,    89,    93,    93,    93,    93,    93,
      93,    93,    93,    91,    92,   142,   151,   151,   151,   151,
       6,     8,   151,   151,   151,   151,    42,    45,    20,    20,
     151,   151,   151,   151,   151,   151,   151,   151,   151,   151,
       6,     8,   170,    12,   151,    43,    43,    91,     6,    89,
     157,    29,    68,     5,    98,    67,   102,    92,   117,   120,
     121,   151,   151,    35,   118,   123,   117,     6,     8,    87,
      87,    87,    87,    89,    89,     6,     8,   147,   148,   170,
      90,   172,    93,    77,    83,    75,   144,   118,   117,    93,
      93,    93,    93,    21,    21,    87,   151,    89,    89,    92,
      93,     6,    78,    91,    92,    93,    82,    89,    91,    19,
      91,    43,    43,    43,    73,    43,    57,    58,    59,   151,
      89,    89,    89,   151,    87,   146,   151,   151,   151,   151,
     151,   151,   151,   151,   174,   143,    45,   151,   151,   151,
      12,    94,   151,   151,    89,     6,    78,   110,   111,    67,
      36,     6,    19,   119,   134,   118,    92,    37,    38,   151,
     118,   140,   140,   140,   140,    88,    88,    91,   173,    89,
       6,     8,   170,    75,   144,    78,    93,   145,   119,   118,
       6,     6,     6,     6,   124,   124,    89,   157,    89,   151,
      82,   151,   164,     6,     6,   151,   151,   151,   151,   151,
     151,   151,   151,    59,    19,    88,    94,   144,   172,   142,
      94,    94,   151,    88,    88,    36,    19,    92,     5,     6,
     151,   121,    89,    89,    89,    89,     6,     8,   147,   170,
     147,    91,    93,   145,     3,    12,    77,     6,     8,   150,
     170,   172,   119,    93,    93,    89,     6,    93,    89,    91,
      19,    19,    19,    92,    19,    59,   151,   151,   146,    89,
      94,   107,   114,   127,   128,   131,   132,   135,   114,     6,
     151,   111,    36,    69,     6,     8,   150,   170,   172,    12,
       3,     3,    88,   145,    88,   145,    88,   145,   145,    94,
       6,     6,     6,   151,   151,   151,     6,   151,    88,    94,
      94,   172,   143,    89,   107,   127,   128,   131,   132,   135,
      89,     6,     5,   145,   145,   145,   145,    94,     3,     6,
       8,    93,   172,     6,     8,    93,   172,     6,     8,    93,
     172,   172,    77,    93,    89,    89,    89,    43,    89,   151,
      89,   142,    69,   172,   172,   172,   172,    77,     6,     8,
      94,     6,     8,    94,     6,     8,    94,    94,    75,     6,
     151,    94,   143,    19,    88,     5,    94,    94,    94,    94,
      75,    77,    77,    77,    77,    88,   142,   151,   151,    77,
      77,    77,    77,    75,    75,    75,    75,   151,    19,    88,
      88,    94,    75,    75,    75,    75,    89,   151,   151,   151,
      88,    94,    94,   151,    94
};

/* YYR1[RULE-NUM] -- Symbol kind of the left-hand side of rule RULE-NUM.  */
static const yytype_uint8 yyr1[] =
{
       0,    95,    96,    96,    96,    96,    97,    97,    97,    98,
      99,    99,   100,   100,   100,   100,   100,   100,   100,   100,
     100,   100,   100,   100,   101,   102,   102,   103,   103,   104,
     104,   104,   105,   105,   106,   106,   107,   107,   108,   108,
     108,   108,   108,   109,   109,   109,   110,   110,   111,   111,
     112,   112,   113,   113,   113,   113,   114,   114,   114,   114,
     114,   114,   114,   114,   114,   114,   114,   114,   115,   115,
     116,   116,   117,   117,   118,   118,   119,   119,   120,   120,
     121,   121,   121,   122,   122,   123,   123,   123,   124,   124,
     124,   124,   125,   125,   125,   125,   125,   126,   126,   126,
     127,   128,   128,   128,   128,   128,   129,   130,   131,   132,
     133,   133,   134,   135,   136,   136,   136,   136,   137,   137,
     137,   137,   137,   138,   138,   139,   139,   140,   140,   141,
     141,   141,   141,   141,   141,   141,   141,   141,   142,   143,
     143,   143,   143,   143,   143,   143,   143,   143,   143,   143,
     143,   143,   143,   143,   143,   143,   143,   143,   143,   143,
     143,   143,   143,   144,   144,   144,   144,   145,   145,   145,
     145,   145,   145,   145,   145,   146,   146,   146,   147,   147,
     147,   148,   148,   149,   149,   149,   149,   149,   149,   150,
     150,   150,   150,   150,   150,   150,   150,   150,   150,   150,
     150,   151,   151,   151,   151,   151,   151,   151,   151,   151,
     151,   151,   151,   151,   151,   151,   151,   151,   151,   151,
     151,   151,   151,   151,   151,   151,   151,   151,   151,   151,
     151,   151,   151,   151,   151,   151,   151,   152,   152,   152,
     152,   152,   152,   152,   152,   152,   152,   152,   152,   152,
     152,   152,   152,   153,   154,   154,   154,   154,   154,   154,
     154,   154,   154,   154,   154,   154,   154,   154,   155,   155,
     155,   155,   156,   157,   157,   158,   158,   159,   159,   159,
     159,   160,   160,   160,   160,   161,   161,   162,   163,   163,
     164,   164,   164,   164,   164,   165,   165,   165,   165,   166,
     166,   167,   168,   168,   168,   168,   168,   168,   169,   169,
     169,   170,   170,   170,   170,   170,   170,   170,   170,   170,
     170,   170,   170,   170,   170,   170,   170,   170,   170,   170,
     170,   170,   170,   170,   170,   170,   170,   170,   170,   170,
     170,   170,   170,   170,   170,   170,   170,   170,   170,   170,
     170,   170,   170,   170,   170,   170,   170,   170,   170,   170,
     171,   172,   172,   172,   173,   173,   174,   174,   174,   174,
     174,   174,   174,   174
};

/* YYR2[RULE-NUM] -- Number of symbols on the right-hand side of rule RULE-NUM.  */
static const yytype_int8 yyr2[] =
{
       0,     2,     1,     2,     2,     3,     1,     3,     4,     1,
       1,     2,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     5,     0,     2,     0,     1,     6,
       5,     6,     7,     6,     4,     4,     8,     8,     4,     3,
       5,     5,     7,     0,     2,     3,     1,     3,     1,     3,
       1,     3,     6,     8,     8,    10,     1,     1,     1,     1,
       1,     1,     2,     2,     2,     2,     2,     2,     0,     1,
       0,     3,     0,     2,     0,     2,     0,     2,     1,     3,
       1,     2,     2,     1,     3,     1,     3,     3,     1,     3,
       5,     7,     1,     3,     3,     3,     3,     3,     3,     3,
       2,     2,     3,     3,     4,     4,     4,     4,     2,     3,
       1,     3,     1,     2,     1,     3,     5,     7,     3,     3,
       3,     3,     3,     1,     0,     1,     3,     1,     3,     1,
       3,     3,     6,     6,     4,     6,     6,     4,     5,     8,
      10,    10,    10,    10,     8,    10,    10,    10,    10,     9,
      11,    11,    11,    11,     7,     9,     9,     9,     9,     3,
       3,     4,     2,     0,     1,     1,     1,     0,     1,     2,
       4,     3,     3,     2,     3,     0,     1,     2,     1,     1,
       1,     3,     3,     2,     2,     2,     3,     3,     3,     3,
       4,     3,     4,     3,     4,     3,     4,     3,     3,     4,
       4,     1,     2,     2,     3,     3,     3,     3,     3,     3,
       3,     3,     3,     3,     3,     3,     3,     3,     3,     3,
       3,     4,     4,     3,     2,     3,     4,     3,     3,     5,
       3,     3,     3,     4,     6,     5,     5,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     3,
       5,     7,     9,     1,     6,     5,     3,     4,     5,     4,
       4,     4,     4,     6,     6,     4,     4,     4,     8,     8,
       8,     8,    12,     1,     3,     2,     3,     5,     7,     7,
       9,    11,    13,    13,    15,     2,     3,     4,     1,     3,
       2,     2,     4,     3,     1,     3,     5,     4,     6,     1,
       2,     4,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     0,     2,     3,     1,     3,     3,     3,     3,     3,
       3,     3,     3,     3
};


/* YYDPREC[RULE-NUM] -- Dynamic precedence of rule #RULE-NUM (0 if none).  */
static const yytype_int8 yydprec[] =
{
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     1,     1,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     2,     2,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     1,     2,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0
};

/* YYMERGER[RULE-NUM] -- Index of merging function for rule #RULE-NUM.  */
static const yytype_int8 yymerger[] =
{
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0
};

/* YYIMMEDIATE[RULE-NUM] -- True iff rule #RULE-NUM is not to be deferred, as
   in the case of predicates.  */
static const yybool yyimmediate[] =
{
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0
};

/* YYCONFLP[YYPACT[STATE-NUM]] -- Pointer into YYCONFL of start of
   list of conflicting reductions corresponding to action entry for
   state STATE-NUM in yytable.  0 means no conflicts.  The list in
   yyconfl is terminated by a rule number of 0.  */
static const yytype_int8 yyconflp[] =
{
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,    13,     0,     0,     0,     0,     0,     0,
      15,    17,     0,     0,    19,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     5,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     7,     0,     0,
       0,     0,     0,     0,     1,     0,     9,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,    21,     0,    23,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,    25,     0,
       0,    27,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,    29,     0,     0,    31,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     3,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,    11,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0
};

/* YYCONFL[I] -- lists of conflicting rule numbers, each terminated by
   0, pointed into by YYCONFLP.  */
static const short yyconfl[] =
{
       0,   308,     0,   163,     0,   308,     0,   308,     0,   308,
       0,   163,     0,   308,     0,   308,     0,   164,     0,   164,
       0,   309,     0,   310,     0,   228,     0,   228,     0,   138,
       0,   138,     0
};


/* YYLLOC_DEFAULT -- Set CURRENT to span from RHS[1] to RHS[N].
   If N is 0, then set CURRENT to the empty location which ends
   the previous symbol: RHS[0] (always defined).  */

#ifndef YYLLOC_DEFAULT
# define YYLLOC_DEFAULT(Current, Rhs, N)                                \
    do                                                                  \
      if (N)                                                            \
        {                                                               \
          (Current).first_line   = YYRHSLOC (Rhs, 1).first_line;        \
          (Current).first_column = YYRHSLOC (Rhs, 1).first_column;      \
          (Current).last_line    = YYRHSLOC (Rhs, N).last_line;         \
          (Current).last_column  = YYRHSLOC (Rhs, N).last_column;       \
        }                                                               \
      else                                                              \
        {                                                               \
          (Current).first_line   = (Current).last_line   =              \
            YYRHSLOC (Rhs, 0).last_line;                                \
          (Current).first_column = (Current).last_column =              \
            YYRHSLOC (Rhs, 0).last_column;                              \
        }                                                               \
    while (0)
#endif

# define YYRHSLOC(Rhs, K) ((Rhs)[K].yystate.yyloc)



#undef yynerrs
#define yynerrs (yystackp->yyerrcnt)
#undef yychar
#define yychar (yystackp->yyrawchar)
#undef yylval
#define yylval (yystackp->yyval)
#undef yylloc
#define yylloc (yystackp->yyloc)
#define cypher_yynerrs yynerrs
#define cypher_yychar yychar
#define cypher_yylval yylval
#define cypher_yylloc yylloc

enum { YYENOMEM = -2 };

typedef enum { yyok, yyaccept, yyabort, yyerr, yynomem } YYRESULTTAG;

#define YYCHK(YYE)                              \
  do {                                          \
    YYRESULTTAG yychk_flag = YYE;               \
    if (yychk_flag != yyok)                     \
      return yychk_flag;                        \
  } while (0)

/* YYINITDEPTH -- initial size of the parser's stacks.  */
#ifndef YYINITDEPTH
# define YYINITDEPTH 200
#endif

/* YYMAXDEPTH -- maximum size the stacks can grow to (effective only
   if the built-in stack extension method is used).

   Do not make this value too large; the results are undefined if
   SIZE_MAX < YYMAXDEPTH * sizeof (GLRStackItem)
   evaluated with infinite-precision integer arithmetic.  */

#ifndef YYMAXDEPTH
# define YYMAXDEPTH 10000
#endif

/* Minimum number of free items on the stack allowed after an
   allocation.  This is to allow allocation and initialization
   to be completed by functions that call yyexpandGLRStack before the
   stack is expanded, thus insuring that all necessary pointers get
   properly redirected to new data.  */
#define YYHEADROOM 2

#ifndef YYSTACKEXPANDABLE
#  define YYSTACKEXPANDABLE 1
#endif

#if YYSTACKEXPANDABLE
# define YY_RESERVE_GLRSTACK(Yystack)                   \
  do {                                                  \
    if (Yystack->yyspaceLeft < YYHEADROOM)              \
      yyexpandGLRStack (Yystack);                       \
  } while (0)
#else
# define YY_RESERVE_GLRSTACK(Yystack)                   \
  do {                                                  \
    if (Yystack->yyspaceLeft < YYHEADROOM)              \
      yyMemoryExhausted (Yystack);                      \
  } while (0)
#endif

/** State numbers. */
typedef int yy_state_t;

/** Rule numbers. */
typedef int yyRuleNum;

/** Item references. */
typedef short yyItemNum;

typedef struct yyGLRState yyGLRState;
typedef struct yyGLRStateSet yyGLRStateSet;
typedef struct yySemanticOption yySemanticOption;
typedef union yyGLRStackItem yyGLRStackItem;
typedef struct yyGLRStack yyGLRStack;

struct yyGLRState
{
  /** Type tag: always true.  */
  yybool yyisState;
  /** Type tag for yysemantics.  If true, yyval applies, otherwise
   *  yyfirstVal applies.  */
  yybool yyresolved;
  /** Number of corresponding LALR(1) machine state.  */
  yy_state_t yylrState;
  /** Preceding state in this stack */
  yyGLRState* yypred;
  /** Source position of the last token produced by my symbol */
  YYPTRDIFF_T yyposn;
  union {
    /** First in a chain of alternative reductions producing the
     *  nonterminal corresponding to this state, threaded through
     *  yynext.  */
    yySemanticOption* yyfirstVal;
    /** Semantic value for this state.  */
    YYSTYPE yyval;
  } yysemantics;
  /** Source location for this state.  */
  YYLTYPE yyloc;
};

struct yyGLRStateSet
{
  yyGLRState** yystates;
  /** During nondeterministic operation, yylookaheadNeeds tracks which
   *  stacks have actually needed the current lookahead.  During deterministic
   *  operation, yylookaheadNeeds[0] is not maintained since it would merely
   *  duplicate yychar != CYPHER_CYPHER_YYEMPTY.  */
  yybool* yylookaheadNeeds;
  YYPTRDIFF_T yysize;
  YYPTRDIFF_T yycapacity;
};

struct yySemanticOption
{
  /** Type tag: always false.  */
  yybool yyisState;
  /** Rule number for this reduction */
  yyRuleNum yyrule;
  /** The last RHS state in the list of states to be reduced.  */
  yyGLRState* yystate;
  /** The lookahead for this reduction.  */
  int yyrawchar;
  YYSTYPE yyval;
  YYLTYPE yyloc;
  /** Next sibling in chain of options.  To facilitate merging,
   *  options are chained in decreasing order by address.  */
  yySemanticOption* yynext;
};

/** Type of the items in the GLR stack.  The yyisState field
 *  indicates which item of the union is valid.  */
union yyGLRStackItem {
  yyGLRState yystate;
  yySemanticOption yyoption;
};

struct yyGLRStack {
  int yyerrState;
  /* To compute the location of the error token.  */
  yyGLRStackItem yyerror_range[3];

  int yyerrcnt;
  int yyrawchar;
  YYSTYPE yyval;
  YYLTYPE yyloc;

  YYJMP_BUF yyexception_buffer;
  yyGLRStackItem* yyitems;
  yyGLRStackItem* yynextFree;
  YYPTRDIFF_T yyspaceLeft;
  yyGLRState* yysplitPoint;
  yyGLRState* yylastDeleted;
  yyGLRStateSet yytops;
};

#if YYSTACKEXPANDABLE
static void yyexpandGLRStack (yyGLRStack* yystackp);
#endif

_Noreturn static void
yyFail (yyGLRStack* yystackp, YYLTYPE *yylocp, cypher_parser_context *context, const char* yymsg)
{
  if (yymsg != YY_NULLPTR)
    yyerror (yylocp, context, yymsg);
  YYLONGJMP (yystackp->yyexception_buffer, 1);
}

_Noreturn static void
yyMemoryExhausted (yyGLRStack* yystackp)
{
  YYLONGJMP (yystackp->yyexception_buffer, 2);
}

/** Accessing symbol of state YYSTATE.  */
static inline yysymbol_kind_t
yy_accessing_symbol (yy_state_t yystate)
{
  return YY_CAST (yysymbol_kind_t, yystos[yystate]);
}

#if 1
/* The user-facing name of the symbol whose (internal) number is
   YYSYMBOL.  No bounds checking.  */
static const char *yysymbol_name (yysymbol_kind_t yysymbol) YY_ATTRIBUTE_UNUSED;

static const char *
yysymbol_name (yysymbol_kind_t yysymbol)
{
  static const char *const yy_sname[] =
  {
  "end of file", "error", "invalid token", "INTEGER", "DECIMAL", "STRING",
  "IDENTIFIER", "PARAMETER", "BQIDENT", "NOT_EQ", "LT_EQ", "GT_EQ",
  "DOT_DOT", "TYPECAST", "PLUS_EQ", "REGEX_MATCH", "MATCH", "RETURN",
  "CREATE", "WHERE", "WITH", "SET", "DELETE", "REMOVE", "MERGE", "UNWIND",
  "DETACH", "FOREACH", "CALL", "YIELD", "OPTIONAL", "DISTINCT", "ORDER",
  "BY", "SKIP", "LIMIT", "AS", "ASC", "DESC", "AND", "OR", "XOR", "NOT",
  "IN", "IS", "NULL_P", "TRUE_P", "FALSE_P", "EXISTS", "ANY", "NONE",
  "SINGLE", "REDUCE", "UNION", "ALL", "CASE", "WHEN", "THEN", "ELSE",
  "END_P", "ON", "SHORTESTPATH", "ALLSHORTESTPATHS", "PATTERN", "EXPLAIN",
  "LOAD", "CSV", "FROM", "HEADERS", "FIELDTERMINATOR", "STARTS", "ENDS",
  "CONTAINS", "'='", "'<'", "'>'", "'+'", "'-'", "'*'", "'/'", "'%'",
  "'^'", "'.'", "'['", "UNARY_MINUS", "UNARY_PLUS", "';'", "'('", "'|'",
  "')'", "'{'", "'}'", "','", "':'", "']'", "$accept", "stmt",
  "union_query", "single_query", "clause_list", "clause", "match_clause",
  "from_graph_opt", "optional_opt", "return_clause", "with_clause",
  "unwind_clause", "foreach_clause", "call_clause", "proc_args_opt",
  "yield_item_list", "yield_item", "procedure_name", "load_csv_clause",
  "foreach_update_list", "distinct_opt", "order_by_opt", "skip_opt",
  "limit_opt", "where_opt", "order_by_list", "order_by_item",
  "return_item_list", "return_item", "set_item_list", "set_target",
  "set_item", "create_clause", "merge_clause", "on_create_clause",
  "on_match_clause", "set_clause", "delete_clause", "delete_item_list",
  "delete_item", "remove_clause", "remove_item_list", "remove_item",
  "detach_opt", "pattern_list", "simple_path", "path", "node_pattern",
  "rel_pattern", "variable_opt", "varlen_range_opt", "label_opt",
  "label_name", "label_alt_list", "label_list", "rel_type_list", "expr",
  "primary_expr", "literal_expr", "function_call", "list_predicate",
  "reduce_expr", "argument_list", "list_literal", "list_comprehension",
  "pattern_comprehension", "map_literal", "map_projection",
  "map_projection_list", "map_projection_item", "case_expression",
  "when_clause_list", "when_clause", "literal", "identifier",
  "non_reserved_kw", "parameter", "properties_opt", "map_pair_list",
  "map_pair", YY_NULLPTR
  };
  return yy_sname[yysymbol];
}
#endif

/** Left-hand-side symbol for rule #YYRULE.  */
static inline yysymbol_kind_t
yylhsNonterm (yyRuleNum yyrule)
{
  return YY_CAST (yysymbol_kind_t, yyr1[yyrule]);
}

#if CYPHER_YYDEBUG

# ifndef YYFPRINTF
#  define YYFPRINTF fprintf
# endif

# define YY_FPRINTF                             \
  YY_IGNORE_USELESS_CAST_BEGIN YY_FPRINTF_

# define YY_FPRINTF_(Args)                      \
  do {                                          \
    YYFPRINTF Args;                             \
    YY_IGNORE_USELESS_CAST_END                  \
  } while (0)

# define YY_DPRINTF                             \
  YY_IGNORE_USELESS_CAST_BEGIN YY_DPRINTF_

# define YY_DPRINTF_(Args)                      \
  do {                                          \
    if (yydebug)                                \
      YYFPRINTF Args;                           \
    YY_IGNORE_USELESS_CAST_END                  \
  } while (0)


/* YYLOCATION_PRINT -- Print the location on the stream.
   This macro was not mandated originally: define only if we know
   we won't break user code: when these are the locations we know.  */

# ifndef YYLOCATION_PRINT

#  if defined YY_LOCATION_PRINT

   /* Temporary convenience wrapper in case some people defined the
      undocumented and private YY_LOCATION_PRINT macros.  */
#   define YYLOCATION_PRINT(File, Loc)  YY_LOCATION_PRINT(File, *(Loc))

#  elif defined CYPHER_YYLTYPE_IS_TRIVIAL && CYPHER_YYLTYPE_IS_TRIVIAL

/* Print *YYLOCP on YYO.  Private, do not rely on its existence. */

YY_ATTRIBUTE_UNUSED
static int
yy_location_print_ (FILE *yyo, YYLTYPE const * const yylocp)
{
  int res = 0;
  int end_col = 0 != yylocp->last_column ? yylocp->last_column - 1 : 0;
  if (0 <= yylocp->first_line)
    {
      res += YYFPRINTF (yyo, "%d", yylocp->first_line);
      if (0 <= yylocp->first_column)
        res += YYFPRINTF (yyo, ".%d", yylocp->first_column);
    }
  if (0 <= yylocp->last_line)
    {
      if (yylocp->first_line < yylocp->last_line)
        {
          res += YYFPRINTF (yyo, "-%d", yylocp->last_line);
          if (0 <= end_col)
            res += YYFPRINTF (yyo, ".%d", end_col);
        }
      else if (0 <= end_col && yylocp->first_column < end_col)
        res += YYFPRINTF (yyo, "-%d", end_col);
    }
  return res;
}

#   define YYLOCATION_PRINT  yy_location_print_

    /* Temporary convenience wrapper in case some people defined the
       undocumented and private YY_LOCATION_PRINT macros.  */
#   define YY_LOCATION_PRINT(File, Loc)  YYLOCATION_PRINT(File, &(Loc))

#  else

#   define YYLOCATION_PRINT(File, Loc) ((void) 0)
    /* Temporary convenience wrapper in case some people defined the
       undocumented and private YY_LOCATION_PRINT macros.  */
#   define YY_LOCATION_PRINT  YYLOCATION_PRINT

#  endif
# endif /* !defined YYLOCATION_PRINT */



/*-----------------------------------.
| Print this symbol's value on YYO.  |
`-----------------------------------*/

static void
yy_symbol_value_print (FILE *yyo,
                       yysymbol_kind_t yykind, YYSTYPE const * const yyvaluep, YYLTYPE const * const yylocationp, cypher_parser_context *context)
{
  FILE *yyoutput = yyo;
  YY_USE (yyoutput);
  YY_USE (yylocationp);
  YY_USE (context);
  if (!yyvaluep)
    return;
  YY_IGNORE_MAYBE_UNINITIALIZED_BEGIN
  YY_USE (yykind);
  YY_IGNORE_MAYBE_UNINITIALIZED_END
}


/*---------------------------.
| Print this symbol on YYO.  |
`---------------------------*/

static void
yy_symbol_print (FILE *yyo,
                 yysymbol_kind_t yykind, YYSTYPE const * const yyvaluep, YYLTYPE const * const yylocationp, cypher_parser_context *context)
{
  YYFPRINTF (yyo, "%s %s (",
             yykind < YYNTOKENS ? "token" : "nterm", yysymbol_name (yykind));

  YYLOCATION_PRINT (yyo, yylocationp);
  YYFPRINTF (yyo, ": ");
  yy_symbol_value_print (yyo, yykind, yyvaluep, yylocationp, context);
  YYFPRINTF (yyo, ")");
}

# define YY_SYMBOL_PRINT(Title, Kind, Value, Location)                  \
  do {                                                                  \
    if (yydebug)                                                        \
      {                                                                 \
        YY_FPRINTF ((stderr, "%s ", Title));                            \
        yy_symbol_print (stderr, Kind, Value, Location, context);        \
        YY_FPRINTF ((stderr, "\n"));                                    \
      }                                                                 \
  } while (0)

static inline void
yy_reduce_print (yybool yynormal, yyGLRStackItem* yyvsp, YYPTRDIFF_T yyk,
                 yyRuleNum yyrule, cypher_parser_context *context);

# define YY_REDUCE_PRINT(Args)          \
  do {                                  \
    if (yydebug)                        \
      yy_reduce_print Args;             \
  } while (0)

/* Nonzero means print parse trace.  It is left uninitialized so that
   multiple parsers can coexist.  */
int yydebug;

static void yypstack (yyGLRStack* yystackp, YYPTRDIFF_T yyk)
  YY_ATTRIBUTE_UNUSED;
static void yypdumpstack (yyGLRStack* yystackp)
  YY_ATTRIBUTE_UNUSED;

#else /* !CYPHER_YYDEBUG */

# define YY_DPRINTF(Args) do {} while (yyfalse)
# define YY_SYMBOL_PRINT(Title, Kind, Value, Location)
# define YY_REDUCE_PRINT(Args)

#endif /* !CYPHER_YYDEBUG */

#ifndef yystrlen
# define yystrlen(S) (YY_CAST (YYPTRDIFF_T, strlen (S)))
#endif

#ifndef yystpcpy
# if defined __GLIBC__ && defined _STRING_H && defined _GNU_SOURCE
#  define yystpcpy stpcpy
# else
/* Copy YYSRC to YYDEST, returning the address of the terminating '\0' in
   YYDEST.  */
static char *
yystpcpy (char *yydest, const char *yysrc)
{
  char *yyd = yydest;
  const char *yys = yysrc;

  while ((*yyd++ = *yys++) != '\0')
    continue;

  return yyd - 1;
}
# endif
#endif



/** Fill in YYVSP[YYLOW1 .. YYLOW0-1] from the chain of states starting
 *  at YYVSP[YYLOW0].yystate.yypred.  Leaves YYVSP[YYLOW1].yystate.yypred
 *  containing the pointer to the next state in the chain.  */
static void yyfillin (yyGLRStackItem *, int, int) YY_ATTRIBUTE_UNUSED;
static void
yyfillin (yyGLRStackItem *yyvsp, int yylow0, int yylow1)
{
  int i;
  yyGLRState *s = yyvsp[yylow0].yystate.yypred;
  for (i = yylow0-1; i >= yylow1; i -= 1)
    {
#if CYPHER_YYDEBUG
      yyvsp[i].yystate.yylrState = s->yylrState;
#endif
      yyvsp[i].yystate.yyresolved = s->yyresolved;
      if (s->yyresolved)
        yyvsp[i].yystate.yysemantics.yyval = s->yysemantics.yyval;
      else
        /* The effect of using yyval or yyloc (in an immediate rule) is
         * undefined.  */
        yyvsp[i].yystate.yysemantics.yyfirstVal = YY_NULLPTR;
      yyvsp[i].yystate.yyloc = s->yyloc;
      s = yyvsp[i].yystate.yypred = s->yypred;
    }
}


/** If yychar is empty, fetch the next token.  */
static inline yysymbol_kind_t
yygetToken (int *yycharp, yyGLRStack* yystackp, cypher_parser_context *context)
{
  yysymbol_kind_t yytoken;
  YY_USE (context);
  if (*yycharp == CYPHER_CYPHER_YYEMPTY)
    {
      YY_DPRINTF ((stderr, "Reading a token\n"));
      *yycharp = yylex (&yylval, &yylloc, context);
    }
  if (*yycharp <= CYPHER_YYEOF)
    {
      *yycharp = CYPHER_YYEOF;
      yytoken = YYSYMBOL_YYEOF;
      YY_DPRINTF ((stderr, "Now at end of input.\n"));
    }
  else
    {
      yytoken = YYTRANSLATE (*yycharp);
      YY_SYMBOL_PRINT ("Next token is", yytoken, &yylval, &yylloc);
    }
  return yytoken;
}

/* Do nothing if YYNORMAL or if *YYLOW <= YYLOW1.  Otherwise, fill in
 * YYVSP[YYLOW1 .. *YYLOW-1] as in yyfillin and set *YYLOW = YYLOW1.
 * For convenience, always return YYLOW1.  */
static inline int yyfill (yyGLRStackItem *, int *, int, yybool)
     YY_ATTRIBUTE_UNUSED;
static inline int
yyfill (yyGLRStackItem *yyvsp, int *yylow, int yylow1, yybool yynormal)
{
  if (!yynormal && yylow1 < *yylow)
    {
      yyfillin (yyvsp, *yylow, yylow1);
      *yylow = yylow1;
    }
  return yylow1;
}

/** Perform user action for rule number YYN, with RHS length YYRHSLEN,
 *  and top stack item YYVSP.  YYLVALP points to place to put semantic
 *  value ($$), and yylocp points to place for location information
 *  (@$).  Returns yyok for normal return, yyaccept for YYACCEPT,
 *  yyerr for YYERROR, yyabort for YYABORT, yynomem for YYNOMEM.  */
static YYRESULTTAG
yyuserAction (yyRuleNum yyrule, int yyrhslen, yyGLRStackItem* yyvsp,
              yyGLRStack* yystackp, YYPTRDIFF_T yyk,
              YYSTYPE* yyvalp, YYLTYPE *yylocp, cypher_parser_context *context)
{
  const yybool yynormal YY_ATTRIBUTE_UNUSED = yystackp->yysplitPoint == YY_NULLPTR;
  int yylow = 1;
  YY_USE (yyvalp);
  YY_USE (yylocp);
  YY_USE (context);
  YY_USE (yyk);
  YY_USE (yyrhslen);
# undef yyerrok
# define yyerrok (yystackp->yyerrState = 0)
# undef YYACCEPT
# define YYACCEPT return yyaccept
# undef YYABORT
# define YYABORT return yyabort
# undef YYNOMEM
# define YYNOMEM return yynomem
# undef YYERROR
# define YYERROR return yyerrok, yyerr
# undef YYRECOVERING
# define YYRECOVERING() (yystackp->yyerrState != 0)
# undef yyclearin
# define yyclearin (yychar = CYPHER_CYPHER_YYEMPTY)
# undef YYFILL
# define YYFILL(N) yyfill (yyvsp, &yylow, (N), yynormal)
# undef YYBACKUP
# define YYBACKUP(Token, Value)                                              \
  return yyerror (yylocp, context, YY_("syntax error: cannot back up")),     \
         yyerrok, yyerr

  if (yyrhslen == 0)
    *yyvalp = yyval_default;
  else
    *yyvalp = yyvsp[YYFILL (1-yyrhslen)].yystate.yysemantics.yyval;
  /* Default location. */
  YYLLOC_DEFAULT ((*yylocp), (yyvsp - yyrhslen), yyrhslen);
  yystackp->yyerror_range[1].yystate.yyloc = *yylocp;
  /* If yyk == -1, we are running a deferred action on a temporary
     stack.  In that case, YY_REDUCE_PRINT must not play with YYFILL,
     so pretend the stack is "normal". */
  YY_REDUCE_PRINT ((yynormal || yyk == -1, yyvsp, yyk, yyrule, context));
  switch (yyrule)
    {
  case 2: /* stmt: union_query  */
#line 221 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node);
            context->result = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node);
        }
#line 2931 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 3: /* stmt: union_query ';'  */
#line 226 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node);
            context->result = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node);
        }
#line 2940 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 4: /* stmt: EXPLAIN union_query  */
#line 231 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* For EXPLAIN with UNION, wrap if needed */
            if ((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node)->type == AST_NODE_QUERY) {
                ((cypher_query*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node))->explain = true;
            }
            ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node);
            context->result = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node);
        }
#line 2953 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 5: /* stmt: EXPLAIN union_query ';'  */
#line 240 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            if ((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node)->type == AST_NODE_QUERY) {
                ((cypher_query*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node))->explain = true;
            }
            ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node);
            context->result = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node);
        }
#line 2965 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 6: /* union_query: single_query  */
#line 251 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node);
        }
#line 2973 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 7: /* union_query: union_query UNION single_query  */
#line 255 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_union((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line);
        }
#line 2981 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 8: /* union_query: union_query UNION ALL single_query  */
#line 259 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_union((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), true, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 2989 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 9: /* single_query: clause_list  */
#line 266 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_query((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list), false);
        }
#line 2997 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 10: /* clause_list: clause  */
#line 273 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 3006 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 11: /* clause_list: clause_list clause  */
#line 278 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list);
        }
#line 3015 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 12: /* clause: match_clause  */
#line 285 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.match); }
#line 3021 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 13: /* clause: return_clause  */
#line 286 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.return_clause); }
#line 3027 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 14: /* clause: with_clause  */
#line 287 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.with_clause); }
#line 3033 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 15: /* clause: unwind_clause  */
#line 288 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 3039 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 16: /* clause: foreach_clause  */
#line 289 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 3045 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 17: /* clause: load_csv_clause  */
#line 290 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 3051 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 18: /* clause: create_clause  */
#line 291 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.create); }
#line 3057 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 19: /* clause: merge_clause  */
#line 292 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.merge); }
#line 3063 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 20: /* clause: set_clause  */
#line 293 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.set); }
#line 3069 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 21: /* clause: delete_clause  */
#line 294 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.delete); }
#line 3075 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 22: /* clause: remove_clause  */
#line 295 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.remove); }
#line 3081 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 23: /* clause: call_clause  */
#line 296 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 3087 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 24: /* match_clause: optional_opt MATCH pattern_list from_graph_opt where_opt  */
#line 302 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).match) = make_cypher_match((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.boolean), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.string));
        }
#line 3095 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 25: /* from_graph_opt: %empty  */
#line 308 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = NULL; }
#line 3101 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 26: /* from_graph_opt: FROM IDENTIFIER  */
#line 310 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).string) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string);
        }
#line 3109 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 27: /* optional_opt: %empty  */
#line 316 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).boolean) = false; }
#line 3115 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 28: /* optional_opt: OPTIONAL  */
#line 317 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).boolean) = true; }
#line 3121 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 29: /* return_clause: RETURN distinct_opt return_item_list order_by_opt skip_opt limit_opt  */
#line 323 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).return_clause) = make_cypher_return((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.boolean), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 3129 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 30: /* return_clause: RETURN '*' order_by_opt skip_opt limit_opt  */
#line 327 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* RETURN * - return all bound variables (items=NULL signals star) */
            ((*yyvalp).return_clause) = make_cypher_return(false, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 3138 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 31: /* return_clause: RETURN DISTINCT '*' order_by_opt skip_opt limit_opt  */
#line 332 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).return_clause) = make_cypher_return(true, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 3146 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 32: /* with_clause: WITH distinct_opt return_item_list order_by_opt skip_opt limit_opt where_opt  */
#line 340 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).with_clause) = make_cypher_with((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.boolean), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 3154 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 33: /* with_clause: WITH '*' order_by_opt skip_opt limit_opt where_opt  */
#line 344 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* WITH * - pass all variables through */
            ((*yyvalp).with_clause) = make_cypher_with(false, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 3163 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 34: /* unwind_clause: UNWIND expr AS IDENTIFIER  */
#line 353 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_unwind((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3172 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 35: /* unwind_clause: UNWIND expr AS BQIDENT  */
#line 358 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Backtick-quoted alias (scanner stripped the backticks). */
            ((*yyvalp).node) = (ast_node*)make_cypher_unwind((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3182 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 36: /* foreach_clause: FOREACH '(' IDENTIFIER IN expr '|' foreach_update_list ')'  */
#line 368 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_foreach((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 3191 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 37: /* foreach_clause: FOREACH '(' BQIDENT IN expr '|' foreach_update_list ')'  */
#line 373 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* T-0329: backtick-quoted loop variable. */
            ((*yyvalp).node) = (ast_node*)make_cypher_foreach((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 3201 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 38: /* call_clause: CALL '{' union_query '}'  */
#line 387 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list *branches = ast_list_create();
            ast_list_append(branches, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node));
            ((*yyvalp).node) = (ast_node*)make_cypher_call_subquery(branches, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 3211 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 39: /* call_clause: CALL procedure_name proc_args_opt  */
#line 393 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* `CALL pageRank()`: the graph algorithms are procedures; any
             * other name is ProcedureNotFound at run time (GQLITE-T-0389). */
            ((*yyvalp).node) = (ast_node*)make_cypher_call_procedure((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list), NULL, false, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.string));
        }
#line 3222 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 40: /* call_clause: CALL procedure_name proc_args_opt YIELD '*'  */
#line 400 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_call_procedure((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), NULL, true, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
        }
#line 3231 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 41: /* call_clause: CALL procedure_name proc_args_opt YIELD yield_item_list  */
#line 405 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_call_procedure((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list), false, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
        }
#line 3240 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 42: /* call_clause: CALL procedure_name proc_args_opt YIELD yield_item_list WHERE expr  */
#line 410 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_call_procedure((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 3249 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 43: /* proc_args_opt: %empty  */
#line 417 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                 { ((*yyvalp).list) = NULL; }
#line 3255 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 44: /* proc_args_opt: '(' ')'  */
#line 418 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                 { ((*yyvalp).list) = ast_list_create(); }
#line 3261 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 45: /* proc_args_opt: '(' argument_list ')'  */
#line 419 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                 { ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list); }
#line 3267 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 46: /* yield_item_list: yield_item  */
#line 424 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.return_item));
        }
#line 3276 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 47: /* yield_item_list: yield_item_list ',' yield_item  */
#line 429 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.return_item));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 3285 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 48: /* yield_item: IDENTIFIER  */
#line 437 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).return_item) = make_return_item((ast_node*)make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3294 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 49: /* yield_item: IDENTIFIER AS IDENTIFIER  */
#line 442 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).return_item) = make_return_item((ast_node*)make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3304 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 50: /* procedure_name: IDENTIFIER  */
#line 451 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        { ((*yyvalp).string) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string); }
#line 3310 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 51: /* procedure_name: procedure_name '.' IDENTIFIER  */
#line 453 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            size_t n = strlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string)) + strlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string)) + 2;
            ((*yyvalp).string) = malloc(n);
            snprintf(((*yyvalp).string), n, "%s.%s", (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3322 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 52: /* load_csv_clause: LOAD CSV FROM STRING AS IDENTIFIER  */
#line 465 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_load_csv((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), false, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3332 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 53: /* load_csv_clause: LOAD CSV WITH HEADERS FROM STRING AS IDENTIFIER  */
#line 471 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_load_csv((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), true, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3342 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 54: /* load_csv_clause: LOAD CSV FROM STRING AS IDENTIFIER FIELDTERMINATOR STRING  */
#line 477 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_load_csv((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3353 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 55: /* load_csv_clause: LOAD CSV WITH HEADERS FROM STRING AS IDENTIFIER FIELDTERMINATOR STRING  */
#line 484 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_cypher_load_csv((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), true, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-9)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3364 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 56: /* foreach_update_list: create_clause  */
#line 495 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.create));
        }
#line 3373 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 57: /* foreach_update_list: set_clause  */
#line 500 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.set));
        }
#line 3382 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 58: /* foreach_update_list: delete_clause  */
#line 505 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.delete));
        }
#line 3391 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 59: /* foreach_update_list: merge_clause  */
#line 510 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.merge));
        }
#line 3400 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 60: /* foreach_update_list: remove_clause  */
#line 515 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.remove));
        }
#line 3409 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 61: /* foreach_update_list: foreach_clause  */
#line 520 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 3418 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 62: /* foreach_update_list: foreach_update_list create_clause  */
#line 525 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.create));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list);
        }
#line 3427 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 63: /* foreach_update_list: foreach_update_list set_clause  */
#line 530 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.set));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list);
        }
#line 3436 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 64: /* foreach_update_list: foreach_update_list delete_clause  */
#line 535 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.delete));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list);
        }
#line 3445 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 65: /* foreach_update_list: foreach_update_list merge_clause  */
#line 540 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.merge));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list);
        }
#line 3454 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 66: /* foreach_update_list: foreach_update_list remove_clause  */
#line 545 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.remove));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list);
        }
#line 3463 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 67: /* foreach_update_list: foreach_update_list foreach_clause  */
#line 550 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list);
        }
#line 3472 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 68: /* distinct_opt: %empty  */
#line 557 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).boolean) = false; }
#line 3478 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 69: /* distinct_opt: DISTINCT  */
#line 558 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).boolean) = true; }
#line 3484 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 70: /* order_by_opt: %empty  */
#line 562 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).list) = NULL; }
#line 3490 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 71: /* order_by_opt: ORDER BY order_by_list  */
#line 563 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                             { ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list); }
#line 3496 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 72: /* skip_opt: %empty  */
#line 567 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).node) = NULL; }
#line 3502 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 73: /* skip_opt: SKIP expr  */
#line 568 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 3508 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 74: /* limit_opt: %empty  */
#line 572 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).node) = NULL; }
#line 3514 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 75: /* limit_opt: LIMIT expr  */
#line 573 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 3520 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 76: /* where_opt: %empty  */
#line 577 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).node) = NULL; }
#line 3526 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 77: /* where_opt: WHERE expr  */
#line 578 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 3532 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 78: /* order_by_list: order_by_item  */
#line 583 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.order_by_item));
        }
#line 3541 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 79: /* order_by_list: order_by_list ',' order_by_item  */
#line 588 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.order_by_item));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 3550 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 80: /* order_by_item: expr  */
#line 595 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).order_by_item) = make_order_by_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), false); /* Default is ASC */ }
#line 3556 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 81: /* order_by_item: expr ASC  */
#line 596 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).order_by_item) = make_order_by_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), false); }
#line 3562 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 82: /* order_by_item: expr DESC  */
#line 597 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).order_by_item) = make_order_by_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), true); }
#line 3568 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 83: /* return_item_list: return_item  */
#line 603 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.return_item));
        }
#line 3577 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 84: /* return_item_list: return_item_list ',' return_item  */
#line 608 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.return_item));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 3586 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 85: /* return_item: expr  */
#line 616 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).return_item) = make_return_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), NULL);
            /* The column of an unaliased item is named as written
             * (`count( * )`, `n.missing IS NULL`). */
            if (((*yyvalp).return_item)) ((*yyvalp).return_item)->source_text = source_text_span(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column,
                                                       (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).last_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).last_column);
        }
#line 3598 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 86: /* return_item: expr AS IDENTIFIER  */
#line 624 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).return_item) = make_return_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3607 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 87: /* return_item: expr AS BQIDENT  */
#line 629 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Backtick-quoted alias (e.g. RETURN x AS `name`) — the
             * scanner has already stripped the surrounding backticks.
             * Same shape as IDENTIFIER. */
            ((*yyvalp).return_item) = make_return_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3619 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 88: /* set_item_list: set_item  */
#line 640 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.set_item));
        }
#line 3628 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 89: /* set_item_list: set_item_list ',' set_item  */
#line 645 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.set_item));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 3637 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 90: /* set_item_list: IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER  */
#line 650 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* SET n:L1:L2 — produce two set_items, one per label. */
            ((*yyvalp).list) = ast_list_create();
            {
              cypher_identifier *v1 = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
              cypher_label_expr *l1 = make_label_expr((ast_node*)v1, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_cypher_set_item((ast_node*)l1, NULL, false));
            }
            {
              cypher_identifier *v2 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
              cypher_label_expr *l2 = make_label_expr((ast_node*)v2, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_cypher_set_item((ast_node*)l2, NULL, false));
            }
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
        }
#line 3657 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 91: /* set_item_list: IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER  */
#line 666 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* SET n:L1:L2:L3 — three labels. */
            ((*yyvalp).list) = ast_list_create();
            {
              cypher_identifier *v1 = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
              cypher_label_expr *l1 = make_label_expr((ast_node*)v1, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_cypher_set_item((ast_node*)l1, NULL, false));
            }
            {
              cypher_identifier *v2 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
              cypher_label_expr *l2 = make_label_expr((ast_node*)v2, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_cypher_set_item((ast_node*)l2, NULL, false));
            }
            {
              cypher_identifier *v3 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
              cypher_label_expr *l3 = make_label_expr((ast_node*)v3, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_cypher_set_item((ast_node*)l3, NULL, false));
            }
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string));
        }
#line 3682 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 92: /* set_target: identifier  */
#line 692 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                   { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.identifier); }
#line 3688 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 93: /* set_target: '(' expr ')'  */
#line 693 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                   { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node); if (((*yyvalp).node)) ((*yyvalp).node)->parenthesized = true; }
#line 3694 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 94: /* set_target: set_target '.' IDENTIFIER  */
#line 695 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_property((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3703 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 95: /* set_target: set_target '.' BQIDENT  */
#line 700 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_property((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3712 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 96: /* set_target: set_target '.' non_reserved_kw  */
#line 705 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_property((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3721 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 97: /* set_item: set_target '=' expr  */
#line 713 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).set_item) = make_cypher_set_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), false);
        }
#line 3729 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 98: /* set_item: set_target PLUS_EQ expr  */
#line 717 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* SET n += {map} — merge properties */
            ((*yyvalp).set_item) = make_cypher_set_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), true);
        }
#line 3738 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 99: /* set_item: IDENTIFIER ':' IDENTIFIER  */
#line 722 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* SET n:Label syntax */
            cypher_identifier *var = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            cypher_label_expr *label = make_label_expr((ast_node*)var, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ((*yyvalp).set_item) = make_cypher_set_item((ast_node*)label, NULL, false);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3751 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 100: /* create_clause: CREATE pattern_list  */
#line 735 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).create) = make_cypher_create((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list));
        }
#line 3759 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 101: /* merge_clause: MERGE pattern_list  */
#line 743 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).merge) = make_cypher_merge((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list), NULL, NULL);
        }
#line 3767 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 102: /* merge_clause: MERGE pattern_list on_create_clause  */
#line 747 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).merge) = make_cypher_merge((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list), NULL);
        }
#line 3775 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 103: /* merge_clause: MERGE pattern_list on_match_clause  */
#line 751 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).merge) = make_cypher_merge((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list));
        }
#line 3783 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 104: /* merge_clause: MERGE pattern_list on_create_clause on_match_clause  */
#line 755 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).merge) = make_cypher_merge((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list));
        }
#line 3791 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 105: /* merge_clause: MERGE pattern_list on_match_clause on_create_clause  */
#line 759 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).merge) = make_cypher_merge((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list));
        }
#line 3799 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 106: /* on_create_clause: ON CREATE SET set_item_list  */
#line 766 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list);
        }
#line 3807 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 107: /* on_match_clause: ON MATCH SET set_item_list  */
#line 773 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list);
        }
#line 3815 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 108: /* set_clause: SET set_item_list  */
#line 781 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).set) = make_cypher_set((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list));
        }
#line 3823 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 109: /* delete_clause: detach_opt DELETE delete_item_list  */
#line 789 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).delete) = make_cypher_delete((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.boolean));
        }
#line 3831 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 110: /* delete_item_list: delete_item  */
#line 796 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.delete_item));
        }
#line 3840 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 111: /* delete_item_list: delete_item_list ',' delete_item  */
#line 801 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.delete_item));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 3849 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 112: /* delete_item: expr  */
#line 809 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* `DELETE n`, or an expression: `DELETE list[0]`, `DELETE m.key`. */
            ((*yyvalp).delete_item) = make_delete_item_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 3858 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 113: /* remove_clause: REMOVE remove_item_list  */
#line 818 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).remove) = make_cypher_remove((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list));
        }
#line 3866 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 114: /* remove_item_list: remove_item  */
#line 825 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.remove_item));
        }
#line 3875 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 115: /* remove_item_list: remove_item_list ',' remove_item  */
#line 830 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.remove_item));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 3884 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 116: /* remove_item_list: IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER  */
#line 835 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* REMOVE n:L1:L2 — produce two remove_items, one per label. */
            ((*yyvalp).list) = ast_list_create();
            {
              cypher_identifier *v1 = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
              cypher_label_expr *l1 = make_label_expr((ast_node*)v1, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_remove_item((ast_node*)l1));
            }
            {
              cypher_identifier *v2 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
              cypher_label_expr *l2 = make_label_expr((ast_node*)v2, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_remove_item((ast_node*)l2));
            }
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
        }
#line 3904 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 117: /* remove_item_list: IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER  */
#line 851 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* REMOVE n:L1:L2:L3 — three labels. */
            ((*yyvalp).list) = ast_list_create();
            {
              cypher_identifier *v1 = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
              cypher_label_expr *l1 = make_label_expr((ast_node*)v1, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_remove_item((ast_node*)l1));
            }
            {
              cypher_identifier *v2 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
              cypher_label_expr *l2 = make_label_expr((ast_node*)v2, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_remove_item((ast_node*)l2));
            }
            {
              cypher_identifier *v3 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
              cypher_label_expr *l3 = make_label_expr((ast_node*)v3, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
              ast_list_append(((*yyvalp).list), (ast_node*)make_remove_item((ast_node*)l3));
            }
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string));
        }
#line 3929 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 118: /* remove_item: IDENTIFIER '.' IDENTIFIER  */
#line 875 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* REMOVE n.property - property access */
            cypher_identifier *base = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            ast_node *prop = (ast_node*)make_property((ast_node*)base, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ((*yyvalp).remove_item) = make_remove_item(prop);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3942 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 119: /* remove_item: IDENTIFIER '.' BQIDENT  */
#line 884 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* REMOVE n.`special-key` - backtick-quoted property */
            cypher_identifier *base = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            ast_node *prop = (ast_node*)make_property((ast_node*)base, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ((*yyvalp).remove_item) = make_remove_item(prop);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3955 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 120: /* remove_item: BQIDENT '.' IDENTIFIER  */
#line 893 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* REMOVE `special-var`.property */
            cypher_identifier *base = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            ast_node *prop = (ast_node*)make_property((ast_node*)base, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ((*yyvalp).remove_item) = make_remove_item(prop);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3968 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 121: /* remove_item: BQIDENT '.' BQIDENT  */
#line 902 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* REMOVE `special-var`.`special-key` */
            cypher_identifier *base = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            ast_node *prop = (ast_node*)make_property((ast_node*)base, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ((*yyvalp).remove_item) = make_remove_item(prop);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3981 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 122: /* remove_item: IDENTIFIER ':' IDENTIFIER  */
#line 911 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* REMOVE n:Label syntax */
            cypher_identifier *var = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            cypher_label_expr *label = make_label_expr((ast_node*)var, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ((*yyvalp).remove_item) = make_remove_item((ast_node*)label);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 3994 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 123: /* detach_opt: DETACH  */
#line 923 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).boolean) = true;
        }
#line 4002 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 124: /* detach_opt: %empty  */
#line 927 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).boolean) = false;
        }
#line 4010 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 125: /* pattern_list: path  */
#line 935 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.path));
        }
#line 4019 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 126: /* pattern_list: pattern_list ',' path  */
#line 940 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.path));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 4028 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 127: /* simple_path: node_pattern  */
#line 949 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list *elements = ast_list_create();
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node_pattern));
            ((*yyvalp).path) = make_path(elements);
        }
#line 4038 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 128: /* simple_path: simple_path rel_pattern node_pattern  */
#line 955 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Create a new list copying elements from the existing path */
            ast_list *new_elements = ast_list_create();
            for (int i = 0; i < (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.path)->elements->count; i++) {
                ast_list_append(new_elements, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.path)->elements->items[i]);
            }
            ast_list_append(new_elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.rel_pattern));
            ast_list_append(new_elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node_pattern));

            /* Note: Let Bison handle memory cleanup of $1 automatically */
            /* Manual freeing during parsing can cause parser state corruption */

            ((*yyvalp).path) = make_path(new_elements);
        }
#line 4057 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 129: /* path: simple_path  */
#line 974 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).path) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.path);
        }
#line 4065 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 130: /* path: IDENTIFIER '=' simple_path  */
#line 978 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).path) = make_path_with_var((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.path)->elements);
            /* Free the anonymous path structure, but keep its elements */
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.path));
        }
#line 4075 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 131: /* path: BQIDENT '=' simple_path  */
#line 984 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* T-0329: backtick-quoted named-path variable. */
            ((*yyvalp).path) = make_path_with_var((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.path)->elements);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.path));
        }
#line 4085 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 132: /* path: IDENTIFIER '=' SHORTESTPATH '(' simple_path ')'  */
#line 990 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_path *sp = make_shortest_path((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path)->elements, PATH_TYPE_SHORTEST);
            sp->var_name = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string);
            ((*yyvalp).path) = sp;
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path));
        }
#line 4096 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 133: /* path: BQIDENT '=' SHORTESTPATH '(' simple_path ')'  */
#line 997 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* T-0329: backtick-quoted variable on shortestPath. */
            cypher_path *sp = make_shortest_path((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path)->elements, PATH_TYPE_SHORTEST);
            sp->var_name = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string);
            ((*yyvalp).path) = sp;
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path));
        }
#line 4108 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 134: /* path: SHORTESTPATH '(' simple_path ')'  */
#line 1005 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).path) = make_shortest_path((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path)->elements, PATH_TYPE_SHORTEST);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path));
        }
#line 4117 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 135: /* path: IDENTIFIER '=' ALLSHORTESTPATHS '(' simple_path ')'  */
#line 1010 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_path *sp = make_shortest_path((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path)->elements, PATH_TYPE_ALL_SHORTEST);
            sp->var_name = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string);
            ((*yyvalp).path) = sp;
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path));
        }
#line 4128 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 136: /* path: BQIDENT '=' ALLSHORTESTPATHS '(' simple_path ')'  */
#line 1017 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* T-0329: backtick-quoted variable on allShortestPaths. */
            cypher_path *sp = make_shortest_path((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path)->elements, PATH_TYPE_ALL_SHORTEST);
            sp->var_name = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string);
            ((*yyvalp).path) = sp;
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path));
        }
#line 4140 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 137: /* path: ALLSHORTESTPATHS '(' simple_path ')'  */
#line 1025 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).path) = make_shortest_path((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path)->elements, PATH_TYPE_ALL_SHORTEST);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.path));
        }
#line 4149 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 138: /* node_pattern: '(' variable_opt label_opt properties_opt ')'  */
#line 1033 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node_pattern) = make_node_pattern((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.map));
        }
#line 4157 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 139: /* rel_pattern: '-' '[' variable_opt varlen_range_opt properties_opt ']' '-' '>'  */
#line 1048 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), NULL, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, true, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range));
        }
#line 4165 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 140: /* rel_pattern: '-' '[' variable_opt ':' IDENTIFIER varlen_range_opt properties_opt ']' '-' '>'  */
#line 1052 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, true, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 4174 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 141: /* rel_pattern: '-' '[' variable_opt ':' BQIDENT varlen_range_opt properties_opt ']' '-' '>'  */
#line 1057 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, true, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 4183 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 142: /* rel_pattern: '-' '[' variable_opt ':' non_reserved_kw varlen_range_opt properties_opt ']' '-' '>'  */
#line 1062 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* T-0326: keyword-named rel types (CONTAINS/STARTS/ENDS/etc.). */
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, true, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 4193 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 143: /* rel_pattern: '-' '[' variable_opt ':' rel_type_list varlen_range_opt properties_opt ']' '-' '>'  */
#line 1068 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_rel_pattern *p = make_rel_pattern_multi_type((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, true);
            if (p) p->varlen = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range);
            ((*yyvalp).rel_pattern) = p;
        }
#line 4203 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 144: /* rel_pattern: '<' '-' '[' variable_opt varlen_range_opt properties_opt ']' '-'  */
#line 1075 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), NULL, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), true, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range));
        }
#line 4211 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 145: /* rel_pattern: '<' '-' '[' variable_opt ':' IDENTIFIER varlen_range_opt properties_opt ']' '-'  */
#line 1079 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), true, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
        }
#line 4220 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 146: /* rel_pattern: '<' '-' '[' variable_opt ':' BQIDENT varlen_range_opt properties_opt ']' '-'  */
#line 1084 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), true, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
        }
#line 4229 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 147: /* rel_pattern: '<' '-' '[' variable_opt ':' non_reserved_kw varlen_range_opt properties_opt ']' '-'  */
#line 1089 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* T-0326: keyword-named rel types (CONTAINS/STARTS/ENDS/etc.). */
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), true, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
        }
#line 4239 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 148: /* rel_pattern: '<' '-' '[' variable_opt ':' rel_type_list varlen_range_opt properties_opt ']' '-'  */
#line 1095 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_rel_pattern *p = make_rel_pattern_multi_type((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), true, false);
            if (p) p->varlen = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range);
            ((*yyvalp).rel_pattern) = p;
        }
#line 4249 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 149: /* rel_pattern: '<' '-' '[' variable_opt varlen_range_opt properties_opt ']' '-' '>'  */
#line 1104 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), NULL, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range));
        }
#line 4257 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 150: /* rel_pattern: '<' '-' '[' variable_opt ':' IDENTIFIER varlen_range_opt properties_opt ']' '-' '>'  */
#line 1108 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 4266 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 151: /* rel_pattern: '<' '-' '[' variable_opt ':' BQIDENT varlen_range_opt properties_opt ']' '-' '>'  */
#line 1113 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 4275 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 152: /* rel_pattern: '<' '-' '[' variable_opt ':' non_reserved_kw varlen_range_opt properties_opt ']' '-' '>'  */
#line 1118 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 4284 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 153: /* rel_pattern: '<' '-' '[' variable_opt ':' rel_type_list varlen_range_opt properties_opt ']' '-' '>'  */
#line 1123 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_rel_pattern *p = make_rel_pattern_multi_type((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.map), false, false);
            if (p) p->varlen = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.varlen_range);
            ((*yyvalp).rel_pattern) = p;
        }
#line 4294 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 154: /* rel_pattern: '-' '[' variable_opt varlen_range_opt properties_opt ']' '-'  */
#line 1130 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), NULL, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), false, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range));
        }
#line 4302 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 155: /* rel_pattern: '-' '[' variable_opt ':' IDENTIFIER varlen_range_opt properties_opt ']' '-'  */
#line 1134 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), false, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
        }
#line 4311 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 156: /* rel_pattern: '-' '[' variable_opt ':' BQIDENT varlen_range_opt properties_opt ']' '-'  */
#line 1139 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), false, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
        }
#line 4320 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 157: /* rel_pattern: '-' '[' variable_opt ':' non_reserved_kw varlen_range_opt properties_opt ']' '-'  */
#line 1144 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* T-0326: keyword-named rel types (CONTAINS/STARTS/ENDS/etc.). */
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), false, false, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
        }
#line 4330 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 158: /* rel_pattern: '-' '[' variable_opt ':' rel_type_list varlen_range_opt properties_opt ']' '-'  */
#line 1150 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_rel_pattern *p = make_rel_pattern_multi_type((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.map), false, false);
            if (p) p->varlen = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.varlen_range);
            ((*yyvalp).rel_pattern) = p;
        }
#line 4340 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 159: /* rel_pattern: '-' '-' '>'  */
#line 1157 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen(NULL, NULL, NULL, false, true, NULL);
        }
#line 4348 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 160: /* rel_pattern: '<' '-' '-'  */
#line 1161 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen(NULL, NULL, NULL, true, false, NULL);
        }
#line 4356 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 161: /* rel_pattern: '<' '-' '-' '>'  */
#line 1165 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Bidirectional bare arrow — equivalent to `--` (any direction).
             * openCypher accepts <--> as an explicit bidirectional form. */
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen(NULL, NULL, NULL, false, false, NULL);
        }
#line 4366 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 162: /* rel_pattern: '-' '-'  */
#line 1171 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).rel_pattern) = make_rel_pattern_varlen(NULL, NULL, NULL, false, false, NULL);
        }
#line 4374 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 163: /* variable_opt: %empty  */
#line 1177 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = NULL; }
#line 4380 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 164: /* variable_opt: IDENTIFIER  */
#line 1178 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string); }
#line 4386 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 165: /* variable_opt: BQIDENT  */
#line 1179 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string); }
#line 4392 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 166: /* variable_opt: END_P  */
#line 1180 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = strdup("end"); }
#line 4398 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 167: /* varlen_range_opt: %empty  */
#line 1186 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        { ((*yyvalp).varlen_range) = NULL; }
#line 4404 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 168: /* varlen_range_opt: '*'  */
#line 1188 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        { ((*yyvalp).varlen_range) = make_varlen_range(1, -1); }
#line 4410 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 169: /* varlen_range_opt: '*' INTEGER  */
#line 1190 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        { ((*yyvalp).varlen_range) = make_varlen_range((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.integer), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.integer)); }
#line 4416 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 170: /* varlen_range_opt: '*' INTEGER DOT_DOT INTEGER  */
#line 1192 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        { ((*yyvalp).varlen_range) = make_varlen_range((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.integer), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.integer)); }
#line 4422 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 171: /* varlen_range_opt: '*' INTEGER DOT_DOT  */
#line 1194 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        { ((*yyvalp).varlen_range) = make_varlen_range((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.integer), -1); }
#line 4428 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 172: /* varlen_range_opt: '*' DOT_DOT INTEGER  */
#line 1196 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        { ((*yyvalp).varlen_range) = make_varlen_range(1, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.integer)); }
#line 4434 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 173: /* varlen_range_opt: '*' DOT_DOT  */
#line 1198 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        { ((*yyvalp).varlen_range) = make_varlen_range(1, -1); }
#line 4440 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 174: /* varlen_range_opt: '*' '-' INTEGER  */
#line 1200 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* `[*-2]` (Match4 [10]) */
            free(context->error_message);
            context->error_message = strdup("SyntaxError: InvalidRelationshipPattern: a variable-length bound cannot be negative");
            context->has_error = true;
            context->error_location = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line;
            context->error_column = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_column;
            ((*yyvalp).varlen_range) = NULL;
            YYERROR;
        }
#line 4455 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 175: /* label_opt: %empty  */
#line 1213 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).list) = NULL; }
#line 4461 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 176: /* label_opt: label_list  */
#line 1214 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list); }
#line 4467 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 177: /* label_opt: ':' label_alt_list  */
#line 1216 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* `:A|B`: one AST_NODE_LIST item; make_node_pattern moves it to
             * label_alternatives (GQLITE-T-0125) */
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)make_list((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line));
        }
#line 4478 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 178: /* label_name: IDENTIFIER  */
#line 1226 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).string) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string); }
#line 4484 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 179: /* label_name: BQIDENT  */
#line 1227 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).string) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string); }
#line 4490 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 180: /* label_name: non_reserved_kw  */
#line 1228 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).string) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string); }
#line 4496 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 181: /* label_alt_list: label_name '|' label_name  */
#line 1233 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line));
            ast_list_append(((*yyvalp).list), (ast_node*)make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4507 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 182: /* label_alt_list: label_alt_list '|' label_name  */
#line 1240 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 4517 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 183: /* label_list: ':' IDENTIFIER  */
#line 1249 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *label = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)label);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4528 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 184: /* label_list: ':' BQIDENT  */
#line 1256 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *label = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)label);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4539 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 185: /* label_list: ':' non_reserved_kw  */
#line 1263 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *label = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)label);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4550 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 186: /* label_list: label_list ':' IDENTIFIER  */
#line 1270 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
            cypher_literal *label = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)label);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4561 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 187: /* label_list: label_list ':' non_reserved_kw  */
#line 1277 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
            cypher_literal *label = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)label);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4572 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 188: /* label_list: label_list ':' BQIDENT  */
#line 1284 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
            cypher_literal *label = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)label);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4583 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 189: /* rel_type_list: IDENTIFIER '|' IDENTIFIER  */
#line 1294 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *type_lit1 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            cypher_literal *type_lit3 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit1);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit3);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4597 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 190: /* rel_type_list: IDENTIFIER '|' ':' IDENTIFIER  */
#line 1304 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Support [:TYPE1|:TYPE2] syntax with colon before second type */
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *type_lit1 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            cypher_literal *type_lit4 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit1);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit4);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4612 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 191: /* rel_type_list: IDENTIFIER '|' BQIDENT  */
#line 1315 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *type_lit1 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            cypher_literal *type_lit3 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit1);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit3);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4626 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 192: /* rel_type_list: IDENTIFIER '|' ':' BQIDENT  */
#line 1325 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Support [:TYPE1|:`backtick-type`] syntax */
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *type_lit1 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            cypher_literal *type_lit4 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit1);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit4);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4641 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 193: /* rel_type_list: BQIDENT '|' IDENTIFIER  */
#line 1336 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *type_lit1 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            cypher_literal *type_lit3 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit1);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit3);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4655 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 194: /* rel_type_list: BQIDENT '|' ':' IDENTIFIER  */
#line 1346 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Support [:`backtick-type`|:TYPE2] syntax */
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *type_lit1 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            cypher_literal *type_lit4 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit1);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit4);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4670 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 195: /* rel_type_list: BQIDENT '|' BQIDENT  */
#line 1357 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *type_lit1 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            cypher_literal *type_lit3 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit1);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit3);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4684 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 196: /* rel_type_list: BQIDENT '|' ':' BQIDENT  */
#line 1367 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Support [:`backtick-type`|:`backtick-type2`] syntax */
            ((*yyvalp).list) = ast_list_create();
            cypher_literal *type_lit1 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            cypher_literal *type_lit4 = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit1);
            ast_list_append(((*yyvalp).list), (ast_node*)type_lit4);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4699 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 197: /* rel_type_list: rel_type_list '|' IDENTIFIER  */
#line 1378 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_literal *type_lit = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)type_lit);
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4710 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 198: /* rel_type_list: rel_type_list '|' BQIDENT  */
#line 1385 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_literal *type_lit = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)type_lit);
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4721 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 199: /* rel_type_list: rel_type_list '|' ':' IDENTIFIER  */
#line 1392 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_literal *type_lit = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list), (ast_node*)type_lit);
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4732 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 200: /* rel_type_list: rel_type_list '|' ':' BQIDENT  */
#line 1399 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_literal *type_lit = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list), (ast_node*)type_lit);
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4743 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 201: /* expr: primary_expr  */
#line 1409 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 4749 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 202: /* expr: '+' expr  */
#line 1410 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                 { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 4755 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 203: /* expr: '-' expr  */
#line 1411 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                  { 
        /* Handle unary minus */
        if ((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node)->type == AST_NODE_LITERAL) {
            cypher_literal *lit = (cypher_literal*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node);
            if (lit->literal_type == LITERAL_INTEGER) {
                if (lit->value.integer == INT64_MIN) {
                    /* -2^63: the scanner's pending INT64_MIN */
                    if (context && context->scanner && context->scanner->int_min_pending > 0)
                        context->scanner->int_min_pending--;
                } else {
                    lit->value.integer = -lit->value.integer;
                }
                ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node);
            } else if (lit->literal_type == LITERAL_DECIMAL) {
                lit->value.decimal = -lit->value.decimal;
                ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node);
            } else {
                /* Non-numeric literal: negate as 0 - lit. */
                ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_SUB,
                        (ast_node*)make_integer_literal(0, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line);
            }
        } else {
            /* Non-literal operand (e.g. -3^2, -x, -(a+b)): build a real
             * negation as 0 - expr; previously the negation was silently
             * dropped. (Precedence2 [4]/[5].) */
            ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_SUB,
                    (ast_node*)make_integer_literal(0, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line);
        }
    }
#line 4789 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 204: /* expr: expr '+' expr  */
#line 1440 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_ADD, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4795 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 205: /* expr: expr '-' expr  */
#line 1441 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_SUB, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4801 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 206: /* expr: expr '*' expr  */
#line 1442 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_MUL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4807 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 207: /* expr: expr '/' expr  */
#line 1443 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_DIV, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4813 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 208: /* expr: expr '%' expr  */
#line 1444 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_MOD, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4819 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 209: /* expr: expr '^' expr  */
#line 1445 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_POW, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4825 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 210: /* expr: expr '=' expr  */
#line 1446 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_EQ, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4831 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 211: /* expr: expr NOT_EQ expr  */
#line 1447 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_NEQ, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4837 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 212: /* expr: expr '<' expr  */
#line 1450 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_LT, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4843 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 213: /* expr: expr '>' expr  */
#line 1451 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_GT, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4849 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 214: /* expr: expr LT_EQ expr  */
#line 1452 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_LTE, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4855 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 215: /* expr: expr GT_EQ expr  */
#line 1453 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_GTE, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4861 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 216: /* expr: expr REGEX_MATCH expr  */
#line 1454 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                            { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_REGEX_MATCH, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4867 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 217: /* expr: expr AND expr  */
#line 1455 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_AND, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4873 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 218: /* expr: expr OR expr  */
#line 1456 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_OR, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4879 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 219: /* expr: expr XOR expr  */
#line 1457 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_XOR, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4885 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 220: /* expr: expr IN expr  */
#line 1458 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_IN, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4891 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 221: /* expr: expr STARTS WITH expr  */
#line 1459 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                         { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_STARTS_WITH, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line); }
#line 4897 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 222: /* expr: expr ENDS WITH expr  */
#line 1460 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_ENDS_WITH, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line); }
#line 4903 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 223: /* expr: expr CONTAINS expr  */
#line 1461 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                                        { ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_CONTAINS, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4909 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 224: /* expr: NOT expr  */
#line 1462 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)make_not_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4915 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 225: /* expr: expr IS NULL_P  */
#line 1463 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                          { ((*yyvalp).node) = (ast_node*)make_null_check((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line); }
#line 4921 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 226: /* expr: expr IS NOT NULL_P  */
#line 1464 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                          { ((*yyvalp).node) = (ast_node*)make_null_check((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), true, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line); }
#line 4927 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 227: /* expr: '(' expr ')'  */
#line 1465 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node); if (((*yyvalp).node)) ((*yyvalp).node)->parenthesized = true; }
#line 4933 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 228: /* expr: node_pattern rel_pattern node_pattern  */
#line 1472 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Build a path from the pattern elements */
            ast_list *elements = ast_list_create();
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.rel_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node_pattern));
            cypher_path *path = make_path(elements);
            /* Wrap in pattern list and create EXISTS expression */
            ast_list *pattern_list = ast_list_create();
            ast_list_append(pattern_list, (ast_node*)path);
            ((*yyvalp).node) = (ast_node*)make_exists_pattern_expr(pattern_list, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 4950 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 229: /* expr: node_pattern rel_pattern node_pattern rel_pattern node_pattern  */
#line 1485 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Chained pattern: (a)-[r1]->(b)-[r2]->(c) */
            ast_list *elements = ast_list_create();
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.node_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.rel_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.rel_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node_pattern));
            cypher_path *path = make_path(elements);
            ast_list *pattern_list = ast_list_create();
            ast_list_append(pattern_list, (ast_node*)path);
            ((*yyvalp).node) = (ast_node*)make_exists_pattern_expr(pattern_list, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
        }
#line 4968 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 230: /* expr: expr '.' IDENTIFIER  */
#line 1499 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_property((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4977 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 231: /* expr: expr '.' BQIDENT  */
#line 1504 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_property((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4986 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 232: /* expr: expr '.' non_reserved_kw  */
#line 1509 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_property((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 4995 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 233: /* expr: expr '[' expr ']'  */
#line 1514 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_subscript((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 5003 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 234: /* expr: expr '[' expr DOT_DOT expr ']'  */
#line 1518 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_slice((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
        }
#line 5011 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 235: /* expr: expr '[' expr DOT_DOT ']'  */
#line 1522 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_slice((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 5019 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 236: /* expr: expr '[' DOT_DOT expr ']'  */
#line 1526 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_slice((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.node), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 5027 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 237: /* primary_expr: literal_expr  */
#line 1532 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5033 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 238: /* primary_expr: identifier  */
#line 1533 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.identifier); }
#line 5039 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 239: /* primary_expr: parameter  */
#line 1534 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.parameter); }
#line 5045 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 240: /* primary_expr: function_call  */
#line 1535 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5051 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 241: /* primary_expr: list_predicate  */
#line 1536 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5057 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 242: /* primary_expr: reduce_expr  */
#line 1537 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5063 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 243: /* primary_expr: list_literal  */
#line 1538 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5069 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 244: /* primary_expr: list_comprehension  */
#line 1539 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                         { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5075 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 245: /* primary_expr: pattern_comprehension  */
#line 1540 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                            { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5081 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 246: /* primary_expr: map_literal  */
#line 1541 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5087 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 247: /* primary_expr: map_projection  */
#line 1542 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5093 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 248: /* primary_expr: case_expression  */
#line 1543 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node); }
#line 5099 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 249: /* primary_expr: IDENTIFIER ':' IDENTIFIER  */
#line 1545 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_identifier *base = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            ((*yyvalp).node) = (ast_node*)make_label_expr((ast_node*)base, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5110 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 250: /* primary_expr: IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER  */
#line 1552 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Conjunctive label expression `n:L1:L2` — emit
             * (n:L1) AND (n:L2). */
            cypher_identifier *b1 = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            cypher_identifier *b2 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            cypher_label_expr *l1 = make_label_expr((ast_node*)b1, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            cypher_label_expr *l2 = make_label_expr((ast_node*)b2, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_AND,
                                            (ast_node*)l1, (ast_node*)l2,
                                            (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5127 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 251: /* primary_expr: IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER  */
#line 1565 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Three-label conjunction. */
            cypher_identifier *b1 = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
            cypher_identifier *b2 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
            cypher_identifier *b3 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
            cypher_label_expr *l1 = make_label_expr((ast_node*)b1, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            cypher_label_expr *l2 = make_label_expr((ast_node*)b2, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            cypher_label_expr *l3 = make_label_expr((ast_node*)b3, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            cypher_binary_op *and12 = make_binary_op(BINARY_OP_AND,
                                                     (ast_node*)l1, (ast_node*)l2,
                                                     (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
            ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_AND,
                                            (ast_node*)and12, (ast_node*)l3,
                                            (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5148 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 252: /* primary_expr: IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER ':' IDENTIFIER  */
#line 1582 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Four-label conjunction (Graph5 [4], e.g. `a:C:A:A:C`).
             * Repeated labels are harmless — each conjunct is an
             * independent EXISTS check. */
            cypher_identifier *b1 = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yyloc).first_line);
            cypher_identifier *b2 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yyloc).first_line);
            cypher_identifier *b3 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yyloc).first_line);
            cypher_identifier *b4 = make_identifier(strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yysemantics.yyval.string)), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yyloc).first_line);
            cypher_label_expr *l1 = make_label_expr((ast_node*)b1, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
            cypher_label_expr *l2 = make_label_expr((ast_node*)b2, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            cypher_label_expr *l3 = make_label_expr((ast_node*)b3, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            cypher_label_expr *l4 = make_label_expr((ast_node*)b4, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            cypher_binary_op *and12 = make_binary_op(BINARY_OP_AND,
                                                     (ast_node*)l1, (ast_node*)l2,
                                                     (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yyloc).first_line);
            cypher_binary_op *and123 = make_binary_op(BINARY_OP_AND,
                                                      (ast_node*)and12, (ast_node*)l3,
                                                      (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yyloc).first_line);
            ((*yyvalp).node) = (ast_node*)make_binary_op(BINARY_OP_AND,
                                            (ast_node*)and123, (ast_node*)l4,
                                            (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string)); free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5176 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 253: /* literal_expr: literal  */
#line 1608 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).node) = (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.literal); }
#line 5182 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 254: /* function_call: IDENTIFIER '.' IDENTIFIER '(' argument_list ')'  */
#line 1616 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            size_t len = strlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string)) + 1 + strlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string)) + 1;
            char *qname = malloc(len);
            if (qname) snprintf(qname, len, "%s.%s", (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            ((*yyvalp).node) = (ast_node*)make_function_call(qname ? qname : (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            if (qname) free(qname);
        }
#line 5196 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 255: /* function_call: IDENTIFIER '.' IDENTIFIER '(' ')'  */
#line 1626 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            size_t len = strlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string)) + 1 + strlen((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string)) + 1;
            char *qname = malloc(len);
            if (qname) snprintf(qname, len, "%s.%s", (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            ast_list *args = ast_list_create();
            ((*yyvalp).node) = (ast_node*)make_function_call(qname ? qname : (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), args, false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            if (qname) free(qname);
        }
#line 5211 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 256: /* function_call: IDENTIFIER '(' ')'  */
#line 1637 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Check if this is EXISTS function call */
            if (strcasecmp((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), "exists") == 0) {
                /* Empty EXISTS() - invalid */
                free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
                cypher_yyerror(&(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc), context, "EXISTS requires an argument");
                YYERROR;
            } else {
                ast_list *args = ast_list_create();
                ((*yyvalp).node) = (ast_node*)make_function_call((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), args, false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
                free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
            }
        }
#line 5229 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 257: /* function_call: IDENTIFIER '(' argument_list ')'  */
#line 1652 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Check if this is EXISTS function call */
            if (strcasecmp((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), "exists") == 0) {
                /* EXISTS with argument list - check first arg */
                if ((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list)->count == 1 && (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list)->items[0] != NULL) {
                    ast_node *arg = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list)->items[0];
                    if (arg->type == AST_NODE_PROPERTY) {
                        ((*yyvalp).node) = (ast_node*)make_exists_property_expr(arg, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
                        (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list)->items[0] = NULL;  /* Transfer ownership */
                        ast_list_free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list));
                    } else {
                        ((*yyvalp).node) = (ast_node*)make_exists_pattern_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
                    }
                } else {
                    cypher_yyerror(&(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc), context, "EXISTS requires exactly one argument");
                    ast_list_free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list));
                    free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
                    YYERROR;
                }
                free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            } else {
                ((*yyvalp).node) = (ast_node*)make_function_call((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
                free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            }
        }
#line 5259 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 258: /* function_call: IDENTIFIER '(' DISTINCT expr ')'  */
#line 1678 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list *args = ast_list_create();
            ast_list_append(args, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node));
            ((*yyvalp).node) = (ast_node*)make_function_call((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string), args, true, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.string));
        }
#line 5270 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 259: /* function_call: IDENTIFIER '(' '*' ')'  */
#line 1685 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list *args = ast_list_create();
            /* For count(*), we'll use a special NULL argument to indicate * */
            ast_list_append(args, NULL);
            ((*yyvalp).node) = (ast_node*)make_function_call((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), args, false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
        }
#line 5282 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 260: /* function_call: EXISTS '(' pattern_list ')'  */
#line 1693 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* EXISTS((pattern)) - check for relationship/path existence */
            ((*yyvalp).node) = (ast_node*)make_exists_pattern_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 5291 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 261: /* function_call: EXISTS '{' pattern_list '}'  */
#line 1698 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Existential subquery brace form: EXISTS { (n)-->() }
             * (ExistentialSubquery1 [1]/[3]). Reuses the pattern-existence
             * transform; correlated outer variables resolve via the
             * EXISTS_TYPE_PATTERN emitter's outer-alias lookup. */
            cypher_exists_expr *ee = make_exists_pattern_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            if (ee) ee->is_subquery = true;
            ((*yyvalp).node) = (ast_node*)ee;
        }
#line 5305 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 262: /* function_call: EXISTS '{' single_query '}'  */
#line 1708 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* GQLITE-T-0139: full existential subquery
             * EXISTS { MATCH (n)-->(m) [WITH ... WHERE ...] RETURN true }
             * (CIP2015-05-13-EXISTS level 3; ExistentialSubquery2/3). */
            ((*yyvalp).node) = (ast_node*)make_exists_query_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 5316 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 263: /* function_call: EXISTS '{' pattern_list WHERE expr '}'  */
#line 1715 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Existential subquery with inner WHERE:
             * EXISTS { (n)-->(m) WHERE n.prop = m.prop }
             * (ExistentialSubquery1 [2]/[4]). The inner predicate is stored
             * on where_clause and folded into the subquery by the transform,
             * which registers the inner pattern variables first. */
            cypher_exists_expr *ee = make_exists_pattern_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yyloc).first_line);
            if (ee) { ee->where_clause = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node); ee->is_subquery = true; }
            ((*yyvalp).node) = (ast_node*)ee;
        }
#line 5331 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 264: /* function_call: EXISTS '(' IDENTIFIER '.' IDENTIFIER ')'  */
#line 1726 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* EXISTS(n.property) - unambiguous property existence check */
            ast_node *var = (ast_node*)make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            ast_node *prop = (ast_node*)make_property(var, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yyloc).first_line);
            ((*yyvalp).node) = (ast_node*)make_exists_property_expr(prop, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.string));
        }
#line 5344 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 265: /* function_call: CONTAINS '(' argument_list ')'  */
#line 1736 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_function_call(strdup("contains"), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 5352 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 266: /* function_call: STARTS '(' argument_list ')'  */
#line 1740 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* startsWith function uses STARTS keyword */
            ((*yyvalp).node) = (ast_node*)make_function_call(strdup("startsWith"), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 5361 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 267: /* function_call: ENDS '(' argument_list ')'  */
#line 1745 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* endsWith function uses ENDS keyword */
            ((*yyvalp).node) = (ast_node*)make_function_call(strdup("endsWith"), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 5370 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 268: /* list_predicate: ALL '(' IDENTIFIER IN expr WHERE expr ')'  */
#line 1754 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_list_predicate(LIST_PRED_ALL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 5379 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 269: /* list_predicate: ANY '(' IDENTIFIER IN expr WHERE expr ')'  */
#line 1759 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_list_predicate(LIST_PRED_ANY, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 5388 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 270: /* list_predicate: NONE '(' IDENTIFIER IN expr WHERE expr ')'  */
#line 1764 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_list_predicate(LIST_PRED_NONE, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 5397 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 271: /* list_predicate: SINGLE '(' IDENTIFIER IN expr WHERE expr ')'  */
#line 1769 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_list_predicate(LIST_PRED_SINGLE, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 5406 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 272: /* reduce_expr: REDUCE '(' IDENTIFIER '=' expr ',' IDENTIFIER IN expr '|' expr ')'  */
#line 1778 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* reduce(acc = initial, x IN list | expression) */
            ((*yyvalp).node) = (ast_node*)make_reduce_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-9)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-11)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-9)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 5417 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 273: /* argument_list: expr  */
#line 1789 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 5426 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 274: /* argument_list: argument_list ',' expr  */
#line 1794 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 5435 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 275: /* list_literal: '[' ']'  */
#line 1803 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_list(ast_list_create(), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line);
        }
#line 5443 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 276: /* list_literal: '[' return_item_list ']'  */
#line 1807 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Reuse return_item_list for comma-separated expressions */
            /* But we need to extract the expressions from return_items */
            ast_list *exprs = ast_list_create();
            for (int i = 0; i < (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list)->count; i++) {
                cypher_return_item *item = (cypher_return_item*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list)->items[i];
                ast_list_append(exprs, item->expr);
                item->expr = NULL;  /* Prevent double-free */
            }
            ast_list_free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list));
            ((*yyvalp).node) = (ast_node*)make_list(exprs, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 5460 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 277: /* list_comprehension: '[' IDENTIFIER IN expr ']'  */
#line 1824 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_list_comprehension((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), NULL, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
        }
#line 5469 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 278: /* list_comprehension: '[' IDENTIFIER IN expr WHERE expr ']'  */
#line 1829 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_list_comprehension((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 5478 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 279: /* list_comprehension: '[' IDENTIFIER IN expr '|' expr ']'  */
#line 1834 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_list_comprehension((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.string));
        }
#line 5487 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 280: /* list_comprehension: '[' IDENTIFIER IN expr WHERE expr '|' expr ']'  */
#line 1839 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_list_comprehension((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.string));
        }
#line 5496 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 281: /* pattern_comprehension: '[' '(' variable_opt label_opt properties_opt ')' rel_pattern node_pattern '|' expr ']'  */
#line 1856 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_node_pattern *first = make_node_pattern((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.map));
            ast_list *elements = ast_list_create();
            ast_list_append(elements, (ast_node*)first);
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.rel_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node_pattern));
            cypher_path *path = make_path(elements);

            ast_list *pattern = ast_list_create();
            ast_list_append(pattern, (ast_node*)path);
            ((*yyvalp).node) = (ast_node*)make_pattern_comprehension(pattern, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-10)].yystate.yyloc).first_line);
        }
#line 5513 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 282: /* pattern_comprehension: '[' '(' variable_opt label_opt properties_opt ')' rel_pattern node_pattern WHERE expr '|' expr ']'  */
#line 1869 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_node_pattern *first = make_node_pattern((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-10)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-9)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yysemantics.yyval.map));
            ast_list *elements = ast_list_create();
            ast_list_append(elements, (ast_node*)first);
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.rel_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.node_pattern));
            cypher_path *path = make_path(elements);

            ast_list *pattern = ast_list_create();
            ast_list_append(pattern, (ast_node*)path);
            ((*yyvalp).node) = (ast_node*)make_pattern_comprehension(pattern, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-12)].yystate.yyloc).first_line);
        }
#line 5530 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 283: /* pattern_comprehension: '[' IDENTIFIER '=' '(' variable_opt label_opt properties_opt ')' rel_pattern node_pattern '|' expr ']'  */
#line 1883 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_node_pattern *first = make_node_pattern((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-7)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.map));
            ast_list *elements = ast_list_create();
            ast_list_append(elements, (ast_node*)first);
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.rel_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node_pattern));
            cypher_path *path = make_path(elements);

            ast_list *pattern = ast_list_create();
            ast_list_append(pattern, (ast_node*)path);
            cypher_pattern_comprehension *pc = make_pattern_comprehension(pattern, NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-12)].yystate.yyloc).first_line);
            pc->path_var = strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-11)].yystate.yysemantics.yyval.string));
            ((*yyvalp).node) = (ast_node*)pc;
        }
#line 5549 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 284: /* pattern_comprehension: '[' IDENTIFIER '=' '(' variable_opt label_opt properties_opt ')' rel_pattern node_pattern WHERE expr '|' expr ']'  */
#line 1898 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_node_pattern *first = make_node_pattern((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-10)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-9)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-8)].yystate.yysemantics.yyval.map));
            ast_list *elements = ast_list_create();
            ast_list_append(elements, (ast_node*)first);
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-6)].yystate.yysemantics.yyval.rel_pattern));
            ast_list_append(elements, (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yysemantics.yyval.node_pattern));
            cypher_path *path = make_path(elements);

            ast_list *pattern = ast_list_create();
            ast_list_append(pattern, (ast_node*)path);
            cypher_pattern_comprehension *pc = make_pattern_comprehension(pattern, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-14)].yystate.yyloc).first_line);
            pc->path_var = strdup((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-13)].yystate.yysemantics.yyval.string));
            ((*yyvalp).node) = (ast_node*)pc;
        }
#line 5568 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 285: /* map_literal: '{' '}'  */
#line 1917 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_map(ast_list_create(), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line);
        }
#line 5576 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 286: /* map_literal: '{' map_pair_list '}'  */
#line 1921 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_map((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 5584 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 287: /* map_projection: IDENTIFIER '{' map_projection_list '}'  */
#line 1929 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            cypher_identifier *base = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            ((*yyvalp).node) = (ast_node*)make_map_projection((ast_node*)base, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
        }
#line 5594 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 288: /* map_projection_list: map_projection_item  */
#line 1938 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 5603 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 289: /* map_projection_list: map_projection_list ',' map_projection_item  */
#line 1943 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 5612 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 290: /* map_projection_item: '.' IDENTIFIER  */
#line 1951 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Shorthand: .prop -> key=prop, property=prop, expr=NULL */
            ((*yyvalp).node) = (ast_node*)make_map_projection_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5622 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 291: /* map_projection_item: '.' '*'  */
#line 1957 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* All properties: .* -> key=NULL, property="*", expr=NULL */
            ((*yyvalp).node) = (ast_node*)make_map_projection_item(NULL, strdup("*"), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line);
        }
#line 5631 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 292: /* map_projection_item: IDENTIFIER ':' '.' IDENTIFIER  */
#line 1962 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Aliased property: alias: .prop */
            ((*yyvalp).node) = (ast_node*)make_map_projection_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.string));
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5642 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 293: /* map_projection_item: IDENTIFIER ':' expr  */
#line 1969 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Computed value: alias: expr */
            ((*yyvalp).node) = (ast_node*)make_map_projection_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string));
        }
#line 5652 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 294: /* map_projection_item: IDENTIFIER  */
#line 1975 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Variable selector: `m {x}` is `m {x: x}` (GQLITE-T-0136) */
            ((*yyvalp).node) = (ast_node*)make_map_projection_item((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), NULL,
                     (ast_node*)make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5663 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 295: /* case_expression: CASE when_clause_list END_P  */
#line 1990 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_case_expr(NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 5671 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 296: /* case_expression: CASE when_clause_list ELSE expr END_P  */
#line 1994 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_case_expr(NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yyloc).first_line);
        }
#line 5679 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 297: /* case_expression: CASE expr when_clause_list END_P  */
#line 1999 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_case_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), NULL, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 5687 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 298: /* case_expression: CASE expr when_clause_list ELSE expr END_P  */
#line 2003 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_case_expr((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-4)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-5)].yystate.yyloc).first_line);
        }
#line 5695 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 299: /* when_clause_list: when_clause  */
#line 2010 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
        }
#line 5704 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 300: /* when_clause_list: when_clause_list when_clause  */
#line 2015 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list);
        }
#line 5713 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 301: /* when_clause: WHEN expr THEN expr  */
#line 2023 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).node) = (ast_node*)make_when_clause((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-3)].yystate.yyloc).first_line);
        }
#line 5721 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 302: /* literal: INTEGER  */
#line 2030 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).literal) = make_integer_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.integer), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
        }
#line 5729 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 303: /* literal: DECIMAL  */
#line 2034 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).literal) = make_decimal_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.decimal), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
        }
#line 5737 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 304: /* literal: STRING  */
#line 2038 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).literal) = make_string_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5746 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 305: /* literal: TRUE_P  */
#line 2043 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).literal) = make_boolean_literal(true, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
        }
#line 5754 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 306: /* literal: FALSE_P  */
#line 2047 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).literal) = make_boolean_literal(false, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
        }
#line 5762 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 307: /* literal: NULL_P  */
#line 2051 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).literal) = make_null_literal((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
        }
#line 5770 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 308: /* identifier: IDENTIFIER  */
#line 2058 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).identifier) = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5779 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 309: /* identifier: BQIDENT  */
#line 2063 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).identifier) = make_identifier((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 5788 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 310: /* identifier: END_P  */
#line 2068 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* Allow 'end' as an identifier - it's only reserved in CASE...END context */
            ((*yyvalp).identifier) = make_identifier(strdup("end"), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
        }
#line 5797 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 311: /* non_reserved_kw: SINGLE  */
#line 2079 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "SINGLE"); }
#line 5803 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 312: /* non_reserved_kw: ANY  */
#line 2080 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "ANY"); }
#line 5809 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 313: /* non_reserved_kw: NONE  */
#line 2081 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "NONE"); }
#line 5815 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 314: /* non_reserved_kw: ALL  */
#line 2082 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "ALL"); }
#line 5821 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 315: /* non_reserved_kw: EXISTS  */
#line 2083 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "EXISTS"); }
#line 5827 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 316: /* non_reserved_kw: CONTAINS  */
#line 2084 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "CONTAINS"); }
#line 5833 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 317: /* non_reserved_kw: STARTS  */
#line 2085 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "STARTS"); }
#line 5839 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 318: /* non_reserved_kw: ENDS  */
#line 2086 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "ENDS"); }
#line 5845 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 319: /* non_reserved_kw: REDUCE  */
#line 2087 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "REDUCE"); }
#line 5851 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 320: /* non_reserved_kw: END_P  */
#line 2088 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "END"); }
#line 5857 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 321: /* non_reserved_kw: ON  */
#line 2089 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "ON"); }
#line 5863 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 322: /* non_reserved_kw: PATTERN  */
#line 2090 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "PATTERN"); }
#line 5869 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 323: /* non_reserved_kw: CSV  */
#line 2091 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "CSV"); }
#line 5875 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 324: /* non_reserved_kw: LOAD  */
#line 2092 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "LOAD"); }
#line 5881 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 325: /* non_reserved_kw: YIELD  */
#line 2093 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "YIELD"); }
#line 5887 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 326: /* non_reserved_kw: IN  */
#line 2094 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "IN"); }
#line 5893 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 327: /* non_reserved_kw: IS  */
#line 2095 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "IS"); }
#line 5899 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 328: /* non_reserved_kw: AS  */
#line 2096 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "AS"); }
#line 5905 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 329: /* non_reserved_kw: OR  */
#line 2097 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "OR"); }
#line 5911 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 330: /* non_reserved_kw: AND  */
#line 2098 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "AND"); }
#line 5917 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 331: /* non_reserved_kw: XOR  */
#line 2099 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "XOR"); }
#line 5923 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 332: /* non_reserved_kw: NOT  */
#line 2100 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "NOT"); }
#line 5929 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 333: /* non_reserved_kw: ORDER  */
#line 2101 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "ORDER"); }
#line 5935 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 334: /* non_reserved_kw: BY  */
#line 2102 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "BY"); }
#line 5941 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 335: /* non_reserved_kw: NULL_P  */
#line 2103 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "NULL"); }
#line 5947 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 336: /* non_reserved_kw: CASE  */
#line 2104 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "CASE"); }
#line 5953 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 337: /* non_reserved_kw: WHEN  */
#line 2105 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "WHEN"); }
#line 5959 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 338: /* non_reserved_kw: THEN  */
#line 2106 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "THEN"); }
#line 5965 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 339: /* non_reserved_kw: ELSE  */
#line 2107 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "ELSE"); }
#line 5971 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 340: /* non_reserved_kw: MATCH  */
#line 2108 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "MATCH"); }
#line 5977 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 341: /* non_reserved_kw: RETURN  */
#line 2109 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "RETURN"); }
#line 5983 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 342: /* non_reserved_kw: CREATE  */
#line 2110 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "CREATE"); }
#line 5989 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 343: /* non_reserved_kw: WHERE  */
#line 2111 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "WHERE"); }
#line 5995 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 344: /* non_reserved_kw: WITH  */
#line 2112 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "WITH"); }
#line 6001 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 345: /* non_reserved_kw: SET  */
#line 2113 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "SET"); }
#line 6007 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 346: /* non_reserved_kw: DELETE  */
#line 2114 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "DELETE"); }
#line 6013 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 347: /* non_reserved_kw: REMOVE  */
#line 2115 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "REMOVE"); }
#line 6019 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 348: /* non_reserved_kw: MERGE  */
#line 2116 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "MERGE"); }
#line 6025 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 349: /* non_reserved_kw: UNWIND  */
#line 2117 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "UNWIND"); }
#line 6031 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 350: /* non_reserved_kw: DETACH  */
#line 2118 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "DETACH"); }
#line 6037 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 351: /* non_reserved_kw: FOREACH  */
#line 2119 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "FOREACH"); }
#line 6043 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 352: /* non_reserved_kw: CALL  */
#line 2120 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "CALL"); }
#line 6049 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 353: /* non_reserved_kw: OPTIONAL  */
#line 2121 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "OPTIONAL"); }
#line 6055 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 354: /* non_reserved_kw: DISTINCT  */
#line 2122 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "DISTINCT"); }
#line 6061 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 355: /* non_reserved_kw: SKIP  */
#line 2123 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "SKIP"); }
#line 6067 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 356: /* non_reserved_kw: LIMIT  */
#line 2124 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "LIMIT"); }
#line 6073 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 357: /* non_reserved_kw: UNION  */
#line 2125 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "UNION"); }
#line 6079 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 358: /* non_reserved_kw: FROM  */
#line 2126 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "FROM"); }
#line 6085 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 359: /* non_reserved_kw: HEADERS  */
#line 2127 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                    { ((*yyvalp).string) = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_column, "HEADERS"); }
#line 6091 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 360: /* parameter: PARAMETER  */
#line 2132 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).parameter) = make_parameter((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yyloc).first_line);
            free((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.string));
        }
#line 6100 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 361: /* properties_opt: %empty  */
#line 2140 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        { ((*yyvalp).map) = NULL; }
#line 6106 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 362: /* properties_opt: '{' '}'  */
#line 2141 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
                        {
            /* Empty `{}` produces an empty-map node so downstream
             * validation distinguishes "no properties block written"
             * (NULL) from "explicit empty map" — the latter counts as
             * a property predicate for re-bind detection. */
            ((*yyvalp).map) = make_map(ast_list_create(), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yyloc).first_line);
        }
#line 6118 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 363: /* properties_opt: '{' map_pair_list '}'  */
#line 2149 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).map) = make_map((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-1)].yystate.yysemantics.yyval.list), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 6126 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 364: /* map_pair_list: map_pair  */
#line 2156 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).list) = ast_list_create();
            ast_list_append(((*yyvalp).list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.map_pair));
        }
#line 6135 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 365: /* map_pair_list: map_pair_list ',' map_pair  */
#line 2161 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ast_list_append((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list), (ast_node*)(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.map_pair));
            ((*yyvalp).list) = (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.list);
        }
#line 6144 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 366: /* map_pair: IDENTIFIER ':' expr  */
#line 2169 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).map_pair) = make_map_pair((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 6152 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 367: /* map_pair: BQIDENT ':' expr  */
#line 2173 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).map_pair) = make_map_pair((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 6160 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 368: /* map_pair: STRING ':' expr  */
#line 2177 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).map_pair) = make_map_pair((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 6168 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 369: /* map_pair: non_reserved_kw ':' expr  */
#line 2181 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).map_pair) = make_map_pair((YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yysemantics.yyval.string), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 6176 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 370: /* map_pair: ASC ':' expr  */
#line 2185 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).map_pair) = make_map_pair("asc", (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 6184 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 371: /* map_pair: TRUE_P ':' expr  */
#line 2189 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            /* `{true: true}`: a keyword as a map key keeps its spelling (Boolean4 [4]). */
            char *k = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_column, "true");
            ((*yyvalp).map_pair) = make_map_pair(k, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            free(k);
        }
#line 6195 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 372: /* map_pair: FALSE_P ':' expr  */
#line 2196 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            char *k = keyword_text_at(context, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_column, "false");
            ((*yyvalp).map_pair) = make_map_pair(k, (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
            free(k);
        }
#line 6205 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;

  case 373: /* map_pair: DESC ':' expr  */
#line 2202 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"
        {
            ((*yyvalp).map_pair) = make_map_pair("desc", (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (0)].yystate.yysemantics.yyval.node), (YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL (-2)].yystate.yyloc).first_line);
        }
#line 6213 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"
    break;


#line 6217 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated/cypher_gram.tab.c"

      default: break;
    }
  YY_SYMBOL_PRINT ("-> $$ =", yylhsNonterm (yyrule), yyvalp, yylocp);

  return yyok;
# undef yyerrok
# undef YYABORT
# undef YYACCEPT
# undef YYNOMEM
# undef YYERROR
# undef YYBACKUP
# undef yyclearin
# undef YYRECOVERING
}


static void
yyuserMerge (int yyn, YYSTYPE* yy0, YYSTYPE* yy1)
{
  YY_USE (yy0);
  YY_USE (yy1);

  switch (yyn)
    {

      default: break;
    }
}

                              /* Bison grammar-table manipulation.  */

/*-----------------------------------------------.
| Release the memory associated to this symbol.  |
`-----------------------------------------------*/

static void
yydestruct (const char *yymsg,
            yysymbol_kind_t yykind, YYSTYPE *yyvaluep, YYLTYPE *yylocationp, cypher_parser_context *context)
{
  YY_USE (yyvaluep);
  YY_USE (yylocationp);
  YY_USE (context);
  if (!yymsg)
    yymsg = "Deleting";
  YY_SYMBOL_PRINT (yymsg, yykind, yyvaluep, yylocationp);

  YY_IGNORE_MAYBE_UNINITIALIZED_BEGIN
  YY_USE (yykind);
  YY_IGNORE_MAYBE_UNINITIALIZED_END
}

/** Number of symbols composing the right hand side of rule #RULE.  */
static inline int
yyrhsLength (yyRuleNum yyrule)
{
  return yyr2[yyrule];
}

static void
yydestroyGLRState (char const *yymsg, yyGLRState *yys, cypher_parser_context *context)
{
  if (yys->yyresolved)
    yydestruct (yymsg, yy_accessing_symbol (yys->yylrState),
                &yys->yysemantics.yyval, &yys->yyloc, context);
  else
    {
#if CYPHER_YYDEBUG
      if (yydebug)
        {
          if (yys->yysemantics.yyfirstVal)
            YY_FPRINTF ((stderr, "%s unresolved", yymsg));
          else
            YY_FPRINTF ((stderr, "%s incomplete", yymsg));
          YY_SYMBOL_PRINT ("", yy_accessing_symbol (yys->yylrState), YY_NULLPTR, &yys->yyloc);
        }
#endif

      if (yys->yysemantics.yyfirstVal)
        {
          yySemanticOption *yyoption = yys->yysemantics.yyfirstVal;
          yyGLRState *yyrh;
          int yyn;
          for (yyrh = yyoption->yystate, yyn = yyrhsLength (yyoption->yyrule);
               yyn > 0;
               yyrh = yyrh->yypred, yyn -= 1)
            yydestroyGLRState (yymsg, yyrh, context);
        }
    }
}

#define yypact_value_is_default(Yyn) \
  ((Yyn) == YYPACT_NINF)

/** True iff LR state YYSTATE has only a default reduction (regardless
 *  of token).  */
static inline yybool
yyisDefaultedState (yy_state_t yystate)
{
  return yypact_value_is_default (yypact[yystate]);
}

/** The default reduction for YYSTATE, assuming it has one.  */
static inline yyRuleNum
yydefaultAction (yy_state_t yystate)
{
  return yydefact[yystate];
}

#define yytable_value_is_error(Yyn) \
  0

/** The action to take in YYSTATE on seeing YYTOKEN.
 *  Result R means
 *    R < 0:  Reduce on rule -R.
 *    R = 0:  Error.
 *    R > 0:  Shift to state R.
 *  Set *YYCONFLICTS to a pointer into yyconfl to a 0-terminated list
 *  of conflicting reductions.
 */
static inline int
yygetLRActions (yy_state_t yystate, yysymbol_kind_t yytoken, const short** yyconflicts)
{
  int yyindex = yypact[yystate] + yytoken;
  if (yytoken == YYSYMBOL_YYerror)
    {
      // This is the error token.
      *yyconflicts = yyconfl;
      return 0;
    }
  else if (yyisDefaultedState (yystate)
           || yyindex < 0 || YYLAST < yyindex || yycheck[yyindex] != yytoken)
    {
      *yyconflicts = yyconfl;
      return -yydefact[yystate];
    }
  else if (! yytable_value_is_error (yytable[yyindex]))
    {
      *yyconflicts = yyconfl + yyconflp[yyindex];
      return yytable[yyindex];
    }
  else
    {
      *yyconflicts = yyconfl + yyconflp[yyindex];
      return 0;
    }
}

/** Compute post-reduction state.
 * \param yystate   the current state
 * \param yysym     the nonterminal to push on the stack
 */
static inline yy_state_t
yyLRgotoState (yy_state_t yystate, yysymbol_kind_t yysym)
{
  int yyr = yypgoto[yysym - YYNTOKENS] + yystate;
  if (0 <= yyr && yyr <= YYLAST && yycheck[yyr] == yystate)
    return yytable[yyr];
  else
    return yydefgoto[yysym - YYNTOKENS];
}

static inline yybool
yyisShiftAction (int yyaction)
{
  return 0 < yyaction;
}

static inline yybool
yyisErrorAction (int yyaction)
{
  return yyaction == 0;
}

                                /* GLRStates */

/** Return a fresh GLRStackItem in YYSTACKP.  The item is an LR state
 *  if YYISSTATE, and otherwise a semantic option.  Callers should call
 *  YY_RESERVE_GLRSTACK afterwards to make sure there is sufficient
 *  headroom.  */

static inline yyGLRStackItem*
yynewGLRStackItem (yyGLRStack* yystackp, yybool yyisState)
{
  yyGLRStackItem* yynewItem = yystackp->yynextFree;
  yystackp->yyspaceLeft -= 1;
  yystackp->yynextFree += 1;
  yynewItem->yystate.yyisState = yyisState;
  return yynewItem;
}

/** Add a new semantic action that will execute the action for rule
 *  YYRULE on the semantic values in YYRHS to the list of
 *  alternative actions for YYSTATE.  Assumes that YYRHS comes from
 *  stack #YYK of *YYSTACKP. */
static void
yyaddDeferredAction (yyGLRStack* yystackp, YYPTRDIFF_T yyk, yyGLRState* yystate,
                     yyGLRState* yyrhs, yyRuleNum yyrule)
{
  yySemanticOption* yynewOption =
    &yynewGLRStackItem (yystackp, yyfalse)->yyoption;
  YY_ASSERT (!yynewOption->yyisState);
  yynewOption->yystate = yyrhs;
  yynewOption->yyrule = yyrule;
  if (yystackp->yytops.yylookaheadNeeds[yyk])
    {
      yynewOption->yyrawchar = yychar;
      yynewOption->yyval = yylval;
      yynewOption->yyloc = yylloc;
    }
  else
    yynewOption->yyrawchar = CYPHER_CYPHER_YYEMPTY;
  yynewOption->yynext = yystate->yysemantics.yyfirstVal;
  yystate->yysemantics.yyfirstVal = yynewOption;

  YY_RESERVE_GLRSTACK (yystackp);
}

                                /* GLRStacks */

/** Initialize YYSET to a singleton set containing an empty stack.  */
static yybool
yyinitStateSet (yyGLRStateSet* yyset)
{
  yyset->yysize = 1;
  yyset->yycapacity = 16;
  yyset->yystates
    = YY_CAST (yyGLRState**,
               YYMALLOC (YY_CAST (YYSIZE_T, yyset->yycapacity)
                         * sizeof yyset->yystates[0]));
  if (! yyset->yystates)
    return yyfalse;
  yyset->yystates[0] = YY_NULLPTR;
  yyset->yylookaheadNeeds
    = YY_CAST (yybool*,
               YYMALLOC (YY_CAST (YYSIZE_T, yyset->yycapacity)
                         * sizeof yyset->yylookaheadNeeds[0]));
  if (! yyset->yylookaheadNeeds)
    {
      YYFREE (yyset->yystates);
      return yyfalse;
    }
  memset (yyset->yylookaheadNeeds,
          0,
          YY_CAST (YYSIZE_T, yyset->yycapacity) * sizeof yyset->yylookaheadNeeds[0]);
  return yytrue;
}

static void yyfreeStateSet (yyGLRStateSet* yyset)
{
  YYFREE (yyset->yystates);
  YYFREE (yyset->yylookaheadNeeds);
}

/** Initialize *YYSTACKP to a single empty stack, with total maximum
 *  capacity for all stacks of YYSIZE.  */
static yybool
yyinitGLRStack (yyGLRStack* yystackp, YYPTRDIFF_T yysize)
{
  yystackp->yyerrState = 0;
  yynerrs = 0;
  yystackp->yyspaceLeft = yysize;
  yystackp->yyitems
    = YY_CAST (yyGLRStackItem*,
               YYMALLOC (YY_CAST (YYSIZE_T, yysize)
                         * sizeof yystackp->yynextFree[0]));
  if (!yystackp->yyitems)
    return yyfalse;
  yystackp->yynextFree = yystackp->yyitems;
  yystackp->yysplitPoint = YY_NULLPTR;
  yystackp->yylastDeleted = YY_NULLPTR;
  return yyinitStateSet (&yystackp->yytops);
}


#if YYSTACKEXPANDABLE
# define YYRELOC(YYFROMITEMS, YYTOITEMS, YYX, YYTYPE)                   \
  &((YYTOITEMS)                                                         \
    - ((YYFROMITEMS) - YY_REINTERPRET_CAST (yyGLRStackItem*, (YYX))))->YYTYPE

/** If *YYSTACKP is expandable, extend it.  WARNING: Pointers into the
    stack from outside should be considered invalid after this call.
    We always expand when there are 1 or fewer items left AFTER an
    allocation, so that we can avoid having external pointers exist
    across an allocation.  */
static void
yyexpandGLRStack (yyGLRStack* yystackp)
{
  yyGLRStackItem* yynewItems;
  yyGLRStackItem* yyp0, *yyp1;
  YYPTRDIFF_T yynewSize;
  YYPTRDIFF_T yyn;
  YYPTRDIFF_T yysize = yystackp->yynextFree - yystackp->yyitems;
  if (YYMAXDEPTH - YYHEADROOM < yysize)
    yyMemoryExhausted (yystackp);
  yynewSize = 2*yysize;
  if (YYMAXDEPTH < yynewSize)
    yynewSize = YYMAXDEPTH;
  yynewItems
    = YY_CAST (yyGLRStackItem*,
               YYMALLOC (YY_CAST (YYSIZE_T, yynewSize)
                         * sizeof yynewItems[0]));
  if (! yynewItems)
    yyMemoryExhausted (yystackp);
  for (yyp0 = yystackp->yyitems, yyp1 = yynewItems, yyn = yysize;
       0 < yyn;
       yyn -= 1, yyp0 += 1, yyp1 += 1)
    {
      *yyp1 = *yyp0;
      if (*YY_REINTERPRET_CAST (yybool *, yyp0))
        {
          yyGLRState* yys0 = &yyp0->yystate;
          yyGLRState* yys1 = &yyp1->yystate;
          if (yys0->yypred != YY_NULLPTR)
            yys1->yypred =
              YYRELOC (yyp0, yyp1, yys0->yypred, yystate);
          if (! yys0->yyresolved && yys0->yysemantics.yyfirstVal != YY_NULLPTR)
            yys1->yysemantics.yyfirstVal =
              YYRELOC (yyp0, yyp1, yys0->yysemantics.yyfirstVal, yyoption);
        }
      else
        {
          yySemanticOption* yyv0 = &yyp0->yyoption;
          yySemanticOption* yyv1 = &yyp1->yyoption;
          if (yyv0->yystate != YY_NULLPTR)
            yyv1->yystate = YYRELOC (yyp0, yyp1, yyv0->yystate, yystate);
          if (yyv0->yynext != YY_NULLPTR)
            yyv1->yynext = YYRELOC (yyp0, yyp1, yyv0->yynext, yyoption);
        }
    }
  if (yystackp->yysplitPoint != YY_NULLPTR)
    yystackp->yysplitPoint = YYRELOC (yystackp->yyitems, yynewItems,
                                      yystackp->yysplitPoint, yystate);

  for (yyn = 0; yyn < yystackp->yytops.yysize; yyn += 1)
    if (yystackp->yytops.yystates[yyn] != YY_NULLPTR)
      yystackp->yytops.yystates[yyn] =
        YYRELOC (yystackp->yyitems, yynewItems,
                 yystackp->yytops.yystates[yyn], yystate);
  YYFREE (yystackp->yyitems);
  yystackp->yyitems = yynewItems;
  yystackp->yynextFree = yynewItems + yysize;
  yystackp->yyspaceLeft = yynewSize - yysize;
}
#endif

static void
yyfreeGLRStack (yyGLRStack* yystackp)
{
  YYFREE (yystackp->yyitems);
  yyfreeStateSet (&yystackp->yytops);
}

/** Assuming that YYS is a GLRState somewhere on *YYSTACKP, update the
 *  splitpoint of *YYSTACKP, if needed, so that it is at least as deep as
 *  YYS.  */
static inline void
yyupdateSplit (yyGLRStack* yystackp, yyGLRState* yys)
{
  if (yystackp->yysplitPoint != YY_NULLPTR && yystackp->yysplitPoint > yys)
    yystackp->yysplitPoint = yys;
}

/** Invalidate stack #YYK in *YYSTACKP.  */
static inline void
yymarkStackDeleted (yyGLRStack* yystackp, YYPTRDIFF_T yyk)
{
  if (yystackp->yytops.yystates[yyk] != YY_NULLPTR)
    yystackp->yylastDeleted = yystackp->yytops.yystates[yyk];
  yystackp->yytops.yystates[yyk] = YY_NULLPTR;
}

/** Undelete the last stack in *YYSTACKP that was marked as deleted.  Can
    only be done once after a deletion, and only when all other stacks have
    been deleted.  */
static void
yyundeleteLastStack (yyGLRStack* yystackp)
{
  if (yystackp->yylastDeleted == YY_NULLPTR || yystackp->yytops.yysize != 0)
    return;
  yystackp->yytops.yystates[0] = yystackp->yylastDeleted;
  yystackp->yytops.yysize = 1;
  YY_DPRINTF ((stderr, "Restoring last deleted stack as stack #0.\n"));
  yystackp->yylastDeleted = YY_NULLPTR;
}

static inline void
yyremoveDeletes (yyGLRStack* yystackp)
{
  YYPTRDIFF_T yyi, yyj;
  yyi = yyj = 0;
  while (yyj < yystackp->yytops.yysize)
    {
      if (yystackp->yytops.yystates[yyi] == YY_NULLPTR)
        {
          if (yyi == yyj)
            YY_DPRINTF ((stderr, "Removing dead stacks.\n"));
          yystackp->yytops.yysize -= 1;
        }
      else
        {
          yystackp->yytops.yystates[yyj] = yystackp->yytops.yystates[yyi];
          /* In the current implementation, it's unnecessary to copy
             yystackp->yytops.yylookaheadNeeds[yyi] since, after
             yyremoveDeletes returns, the parser immediately either enters
             deterministic operation or shifts a token.  However, it doesn't
             hurt, and the code might evolve to need it.  */
          yystackp->yytops.yylookaheadNeeds[yyj] =
            yystackp->yytops.yylookaheadNeeds[yyi];
          if (yyj != yyi)
            YY_DPRINTF ((stderr, "Rename stack %ld -> %ld.\n",
                        YY_CAST (long, yyi), YY_CAST (long, yyj)));
          yyj += 1;
        }
      yyi += 1;
    }
}

/** Shift to a new state on stack #YYK of *YYSTACKP, corresponding to LR
 * state YYLRSTATE, at input position YYPOSN, with (resolved) semantic
 * value *YYVALP and source location *YYLOCP.  */
static inline void
yyglrShift (yyGLRStack* yystackp, YYPTRDIFF_T yyk, yy_state_t yylrState,
            YYPTRDIFF_T yyposn,
            YYSTYPE* yyvalp, YYLTYPE* yylocp)
{
  yyGLRState* yynewState = &yynewGLRStackItem (yystackp, yytrue)->yystate;

  yynewState->yylrState = yylrState;
  yynewState->yyposn = yyposn;
  yynewState->yyresolved = yytrue;
  yynewState->yypred = yystackp->yytops.yystates[yyk];
  yynewState->yysemantics.yyval = *yyvalp;
  yynewState->yyloc = *yylocp;
  yystackp->yytops.yystates[yyk] = yynewState;

  YY_RESERVE_GLRSTACK (yystackp);
}

/** Shift stack #YYK of *YYSTACKP, to a new state corresponding to LR
 *  state YYLRSTATE, at input position YYPOSN, with the (unresolved)
 *  semantic value of YYRHS under the action for YYRULE.  */
static inline void
yyglrShiftDefer (yyGLRStack* yystackp, YYPTRDIFF_T yyk, yy_state_t yylrState,
                 YYPTRDIFF_T yyposn, yyGLRState* yyrhs, yyRuleNum yyrule)
{
  yyGLRState* yynewState = &yynewGLRStackItem (yystackp, yytrue)->yystate;
  YY_ASSERT (yynewState->yyisState);

  yynewState->yylrState = yylrState;
  yynewState->yyposn = yyposn;
  yynewState->yyresolved = yyfalse;
  yynewState->yypred = yystackp->yytops.yystates[yyk];
  yynewState->yysemantics.yyfirstVal = YY_NULLPTR;
  yystackp->yytops.yystates[yyk] = yynewState;

  /* Invokes YY_RESERVE_GLRSTACK.  */
  yyaddDeferredAction (yystackp, yyk, yynewState, yyrhs, yyrule);
}

#if CYPHER_YYDEBUG

/*----------------------------------------------------------------------.
| Report that stack #YYK of *YYSTACKP is going to be reduced by YYRULE. |
`----------------------------------------------------------------------*/

static inline void
yy_reduce_print (yybool yynormal, yyGLRStackItem* yyvsp, YYPTRDIFF_T yyk,
                 yyRuleNum yyrule, cypher_parser_context *context)
{
  int yynrhs = yyrhsLength (yyrule);
  int yylow = 1;
  int yyi;
  YY_FPRINTF ((stderr, "Reducing stack %ld by rule %d (line %d):\n",
               YY_CAST (long, yyk), yyrule - 1, yyrline[yyrule]));
  if (! yynormal)
    yyfillin (yyvsp, 1, -yynrhs);
  /* The symbols being reduced.  */
  for (yyi = 0; yyi < yynrhs; yyi++)
    {
      YY_FPRINTF ((stderr, "   $%d = ", yyi + 1));
      yy_symbol_print (stderr,
                       yy_accessing_symbol (yyvsp[yyi - yynrhs + 1].yystate.yylrState),
                       &yyvsp[yyi - yynrhs + 1].yystate.yysemantics.yyval,
                       &(YY_CAST (yyGLRStackItem const *, yyvsp)[YYFILL ((yyi + 1) - (yynrhs))].yystate.yyloc)                       , context);
      if (!yyvsp[yyi - yynrhs + 1].yystate.yyresolved)
        YY_FPRINTF ((stderr, " (unresolved)"));
      YY_FPRINTF ((stderr, "\n"));
    }
}
#endif

/** Pop the symbols consumed by reduction #YYRULE from the top of stack
 *  #YYK of *YYSTACKP, and perform the appropriate semantic action on their
 *  semantic values.  Assumes that all ambiguities in semantic values
 *  have been previously resolved.  Set *YYVALP to the resulting value,
 *  and *YYLOCP to the computed location (if any).  Return value is as
 *  for userAction.  */
static inline YYRESULTTAG
yydoAction (yyGLRStack* yystackp, YYPTRDIFF_T yyk, yyRuleNum yyrule,
            YYSTYPE* yyvalp, YYLTYPE *yylocp, cypher_parser_context *context)
{
  int yynrhs = yyrhsLength (yyrule);

  if (yystackp->yysplitPoint == YY_NULLPTR)
    {
      /* Standard special case: single stack.  */
      yyGLRStackItem* yyrhs
        = YY_REINTERPRET_CAST (yyGLRStackItem*, yystackp->yytops.yystates[yyk]);
      YY_ASSERT (yyk == 0);
      yystackp->yynextFree -= yynrhs;
      yystackp->yyspaceLeft += yynrhs;
      yystackp->yytops.yystates[0] = & yystackp->yynextFree[-1].yystate;
      return yyuserAction (yyrule, yynrhs, yyrhs, yystackp, yyk,
                           yyvalp, yylocp, context);
    }
  else
    {
      yyGLRStackItem yyrhsVals[YYMAXRHS + YYMAXLEFT + 1];
      yyGLRState* yys = yyrhsVals[YYMAXRHS + YYMAXLEFT].yystate.yypred
        = yystackp->yytops.yystates[yyk];
      int yyi;
      if (yynrhs == 0)
        /* Set default location.  */
        yyrhsVals[YYMAXRHS + YYMAXLEFT - 1].yystate.yyloc = yys->yyloc;
      for (yyi = 0; yyi < yynrhs; yyi += 1)
        {
          yys = yys->yypred;
          YY_ASSERT (yys);
        }
      yyupdateSplit (yystackp, yys);
      yystackp->yytops.yystates[yyk] = yys;
      return yyuserAction (yyrule, yynrhs, yyrhsVals + YYMAXRHS + YYMAXLEFT - 1,
                           yystackp, yyk, yyvalp, yylocp, context);
    }
}

/** Pop items off stack #YYK of *YYSTACKP according to grammar rule YYRULE,
 *  and push back on the resulting nonterminal symbol.  Perform the
 *  semantic action associated with YYRULE and store its value with the
 *  newly pushed state, if YYFORCEEVAL or if *YYSTACKP is currently
 *  unambiguous.  Otherwise, store the deferred semantic action with
 *  the new state.  If the new state would have an identical input
 *  position, LR state, and predecessor to an existing state on the stack,
 *  it is identified with that existing state, eliminating stack #YYK from
 *  *YYSTACKP.  In this case, the semantic value is
 *  added to the options for the existing state's semantic value.
 */
static inline YYRESULTTAG
yyglrReduce (yyGLRStack* yystackp, YYPTRDIFF_T yyk, yyRuleNum yyrule,
             yybool yyforceEval, cypher_parser_context *context)
{
  YYPTRDIFF_T yyposn = yystackp->yytops.yystates[yyk]->yyposn;

  if (yyforceEval || yystackp->yysplitPoint == YY_NULLPTR)
    {
      YYSTYPE yyval;
      YYLTYPE yyloc;

      YYRESULTTAG yyflag = yydoAction (yystackp, yyk, yyrule, &yyval, &yyloc, context);
      if (yyflag == yyerr && yystackp->yysplitPoint != YY_NULLPTR)
        YY_DPRINTF ((stderr,
                     "Parse on stack %ld rejected by rule %d (line %d).\n",
                     YY_CAST (long, yyk), yyrule - 1, yyrline[yyrule]));
      if (yyflag != yyok)
        return yyflag;
      yyglrShift (yystackp, yyk,
                  yyLRgotoState (yystackp->yytops.yystates[yyk]->yylrState,
                                 yylhsNonterm (yyrule)),
                  yyposn, &yyval, &yyloc);
    }
  else
    {
      YYPTRDIFF_T yyi;
      int yyn;
      yyGLRState* yys, *yys0 = yystackp->yytops.yystates[yyk];
      yy_state_t yynewLRState;

      for (yys = yystackp->yytops.yystates[yyk], yyn = yyrhsLength (yyrule);
           0 < yyn; yyn -= 1)
        {
          yys = yys->yypred;
          YY_ASSERT (yys);
        }
      yyupdateSplit (yystackp, yys);
      yynewLRState = yyLRgotoState (yys->yylrState, yylhsNonterm (yyrule));
      YY_DPRINTF ((stderr,
                   "Reduced stack %ld by rule %d (line %d); action deferred.  "
                   "Now in state %d.\n",
                   YY_CAST (long, yyk), yyrule - 1, yyrline[yyrule],
                   yynewLRState));
      for (yyi = 0; yyi < yystackp->yytops.yysize; yyi += 1)
        if (yyi != yyk && yystackp->yytops.yystates[yyi] != YY_NULLPTR)
          {
            yyGLRState *yysplit = yystackp->yysplitPoint;
            yyGLRState *yyp = yystackp->yytops.yystates[yyi];
            while (yyp != yys && yyp != yysplit && yyp->yyposn >= yyposn)
              {
                if (yyp->yylrState == yynewLRState && yyp->yypred == yys)
                  {
                    yyaddDeferredAction (yystackp, yyk, yyp, yys0, yyrule);
                    yymarkStackDeleted (yystackp, yyk);
                    YY_DPRINTF ((stderr, "Merging stack %ld into stack %ld.\n",
                                 YY_CAST (long, yyk), YY_CAST (long, yyi)));
                    return yyok;
                  }
                yyp = yyp->yypred;
              }
          }
      yystackp->yytops.yystates[yyk] = yys;
      yyglrShiftDefer (yystackp, yyk, yynewLRState, yyposn, yys0, yyrule);
    }
  return yyok;
}

static YYPTRDIFF_T
yysplitStack (yyGLRStack* yystackp, YYPTRDIFF_T yyk)
{
  if (yystackp->yysplitPoint == YY_NULLPTR)
    {
      YY_ASSERT (yyk == 0);
      yystackp->yysplitPoint = yystackp->yytops.yystates[yyk];
    }
  if (yystackp->yytops.yycapacity <= yystackp->yytops.yysize)
    {
      YYPTRDIFF_T state_size = YYSIZEOF (yystackp->yytops.yystates[0]);
      YYPTRDIFF_T half_max_capacity = YYSIZE_MAXIMUM / 2 / state_size;
      if (half_max_capacity < yystackp->yytops.yycapacity)
        yyMemoryExhausted (yystackp);
      yystackp->yytops.yycapacity *= 2;

      {
        yyGLRState** yynewStates
          = YY_CAST (yyGLRState**,
                     YYREALLOC (yystackp->yytops.yystates,
                                (YY_CAST (YYSIZE_T, yystackp->yytops.yycapacity)
                                 * sizeof yynewStates[0])));
        if (yynewStates == YY_NULLPTR)
          yyMemoryExhausted (yystackp);
        yystackp->yytops.yystates = yynewStates;
      }

      {
        yybool* yynewLookaheadNeeds
          = YY_CAST (yybool*,
                     YYREALLOC (yystackp->yytops.yylookaheadNeeds,
                                (YY_CAST (YYSIZE_T, yystackp->yytops.yycapacity)
                                 * sizeof yynewLookaheadNeeds[0])));
        if (yynewLookaheadNeeds == YY_NULLPTR)
          yyMemoryExhausted (yystackp);
        yystackp->yytops.yylookaheadNeeds = yynewLookaheadNeeds;
      }
    }
  yystackp->yytops.yystates[yystackp->yytops.yysize]
    = yystackp->yytops.yystates[yyk];
  yystackp->yytops.yylookaheadNeeds[yystackp->yytops.yysize]
    = yystackp->yytops.yylookaheadNeeds[yyk];
  yystackp->yytops.yysize += 1;
  return yystackp->yytops.yysize - 1;
}

/** True iff YYY0 and YYY1 represent identical options at the top level.
 *  That is, they represent the same rule applied to RHS symbols
 *  that produce the same terminal symbols.  */
static yybool
yyidenticalOptions (yySemanticOption* yyy0, yySemanticOption* yyy1)
{
  if (yyy0->yyrule == yyy1->yyrule)
    {
      yyGLRState *yys0, *yys1;
      int yyn;
      for (yys0 = yyy0->yystate, yys1 = yyy1->yystate,
           yyn = yyrhsLength (yyy0->yyrule);
           yyn > 0;
           yys0 = yys0->yypred, yys1 = yys1->yypred, yyn -= 1)
        if (yys0->yyposn != yys1->yyposn)
          return yyfalse;
      return yytrue;
    }
  else
    return yyfalse;
}

/** Assuming identicalOptions (YYY0,YYY1), destructively merge the
 *  alternative semantic values for the RHS-symbols of YYY1 and YYY0.  */
static void
yymergeOptionSets (yySemanticOption* yyy0, yySemanticOption* yyy1)
{
  yyGLRState *yys0, *yys1;
  int yyn;
  for (yys0 = yyy0->yystate, yys1 = yyy1->yystate,
       yyn = yyrhsLength (yyy0->yyrule);
       0 < yyn;
       yys0 = yys0->yypred, yys1 = yys1->yypred, yyn -= 1)
    {
      if (yys0 == yys1)
        break;
      else if (yys0->yyresolved)
        {
          yys1->yyresolved = yytrue;
          yys1->yysemantics.yyval = yys0->yysemantics.yyval;
        }
      else if (yys1->yyresolved)
        {
          yys0->yyresolved = yytrue;
          yys0->yysemantics.yyval = yys1->yysemantics.yyval;
        }
      else
        {
          yySemanticOption** yyz0p = &yys0->yysemantics.yyfirstVal;
          yySemanticOption* yyz1 = yys1->yysemantics.yyfirstVal;
          while (yytrue)
            {
              if (yyz1 == *yyz0p || yyz1 == YY_NULLPTR)
                break;
              else if (*yyz0p == YY_NULLPTR)
                {
                  *yyz0p = yyz1;
                  break;
                }
              else if (*yyz0p < yyz1)
                {
                  yySemanticOption* yyz = *yyz0p;
                  *yyz0p = yyz1;
                  yyz1 = yyz1->yynext;
                  (*yyz0p)->yynext = yyz;
                }
              yyz0p = &(*yyz0p)->yynext;
            }
          yys1->yysemantics.yyfirstVal = yys0->yysemantics.yyfirstVal;
        }
    }
}

/** Y0 and Y1 represent two possible actions to take in a given
 *  parsing state; return 0 if no combination is possible,
 *  1 if user-mergeable, 2 if Y0 is preferred, 3 if Y1 is preferred.  */
static int
yypreference (yySemanticOption* y0, yySemanticOption* y1)
{
  yyRuleNum r0 = y0->yyrule, r1 = y1->yyrule;
  int p0 = yydprec[r0], p1 = yydprec[r1];

  if (p0 == p1)
    {
      if (yymerger[r0] == 0 || yymerger[r0] != yymerger[r1])
        return 0;
      else
        return 1;
    }
  if (p0 == 0 || p1 == 0)
    return 0;
  if (p0 < p1)
    return 3;
  if (p1 < p0)
    return 2;
  return 0;
}

static YYRESULTTAG
yyresolveValue (yyGLRState* yys, yyGLRStack* yystackp, cypher_parser_context *context);


/** Resolve the previous YYN states starting at and including state YYS
 *  on *YYSTACKP. If result != yyok, some states may have been left
 *  unresolved possibly with empty semantic option chains.  Regardless
 *  of whether result = yyok, each state has been left with consistent
 *  data so that yydestroyGLRState can be invoked if necessary.  */
static YYRESULTTAG
yyresolveStates (yyGLRState* yys, int yyn,
                 yyGLRStack* yystackp, cypher_parser_context *context)
{
  if (0 < yyn)
    {
      YY_ASSERT (yys->yypred);
      YYCHK (yyresolveStates (yys->yypred, yyn-1, yystackp, context));
      if (! yys->yyresolved)
        YYCHK (yyresolveValue (yys, yystackp, context));
    }
  return yyok;
}

/** Resolve the states for the RHS of YYOPT on *YYSTACKP, perform its
 *  user action, and return the semantic value and location in *YYVALP
 *  and *YYLOCP.  Regardless of whether result = yyok, all RHS states
 *  have been destroyed (assuming the user action destroys all RHS
 *  semantic values if invoked).  */
static YYRESULTTAG
yyresolveAction (yySemanticOption* yyopt, yyGLRStack* yystackp,
                 YYSTYPE* yyvalp, YYLTYPE *yylocp, cypher_parser_context *context)
{
  yyGLRStackItem yyrhsVals[YYMAXRHS + YYMAXLEFT + 1];
  int yynrhs = yyrhsLength (yyopt->yyrule);
  YYRESULTTAG yyflag =
    yyresolveStates (yyopt->yystate, yynrhs, yystackp, context);
  if (yyflag != yyok)
    {
      yyGLRState *yys;
      for (yys = yyopt->yystate; yynrhs > 0; yys = yys->yypred, yynrhs -= 1)
        yydestroyGLRState ("Cleanup: popping", yys, context);
      return yyflag;
    }

  yyrhsVals[YYMAXRHS + YYMAXLEFT].yystate.yypred = yyopt->yystate;
  if (yynrhs == 0)
    /* Set default location.  */
    yyrhsVals[YYMAXRHS + YYMAXLEFT - 1].yystate.yyloc = yyopt->yystate->yyloc;
  {
    int yychar_current = yychar;
    YYSTYPE yylval_current = yylval;
    YYLTYPE yylloc_current = yylloc;
    yychar = yyopt->yyrawchar;
    yylval = yyopt->yyval;
    yylloc = yyopt->yyloc;
    yyflag = yyuserAction (yyopt->yyrule, yynrhs,
                           yyrhsVals + YYMAXRHS + YYMAXLEFT - 1,
                           yystackp, -1, yyvalp, yylocp, context);
    yychar = yychar_current;
    yylval = yylval_current;
    yylloc = yylloc_current;
  }
  return yyflag;
}

#if CYPHER_YYDEBUG
static void
yyreportTree (yySemanticOption* yyx, int yyindent)
{
  int yynrhs = yyrhsLength (yyx->yyrule);
  int yyi;
  yyGLRState* yys;
  yyGLRState* yystates[1 + YYMAXRHS];
  yyGLRState yyleftmost_state;

  for (yyi = yynrhs, yys = yyx->yystate; 0 < yyi; yyi -= 1, yys = yys->yypred)
    yystates[yyi] = yys;
  if (yys == YY_NULLPTR)
    {
      yyleftmost_state.yyposn = 0;
      yystates[0] = &yyleftmost_state;
    }
  else
    yystates[0] = yys;

  if (yyx->yystate->yyposn < yys->yyposn + 1)
    YY_FPRINTF ((stderr, "%*s%s -> <Rule %d, empty>\n",
                 yyindent, "", yysymbol_name (yylhsNonterm (yyx->yyrule)),
                 yyx->yyrule - 1));
  else
    YY_FPRINTF ((stderr, "%*s%s -> <Rule %d, tokens %ld .. %ld>\n",
                 yyindent, "", yysymbol_name (yylhsNonterm (yyx->yyrule)),
                 yyx->yyrule - 1, YY_CAST (long, yys->yyposn + 1),
                 YY_CAST (long, yyx->yystate->yyposn)));
  for (yyi = 1; yyi <= yynrhs; yyi += 1)
    {
      if (yystates[yyi]->yyresolved)
        {
          if (yystates[yyi-1]->yyposn+1 > yystates[yyi]->yyposn)
            YY_FPRINTF ((stderr, "%*s%s <empty>\n", yyindent+2, "",
                         yysymbol_name (yy_accessing_symbol (yystates[yyi]->yylrState))));
          else
            YY_FPRINTF ((stderr, "%*s%s <tokens %ld .. %ld>\n", yyindent+2, "",
                         yysymbol_name (yy_accessing_symbol (yystates[yyi]->yylrState)),
                         YY_CAST (long, yystates[yyi-1]->yyposn + 1),
                         YY_CAST (long, yystates[yyi]->yyposn)));
        }
      else
        yyreportTree (yystates[yyi]->yysemantics.yyfirstVal, yyindent+2);
    }
}
#endif

static YYRESULTTAG
yyreportAmbiguity (yySemanticOption* yyx0,
                   yySemanticOption* yyx1, YYLTYPE *yylocp, cypher_parser_context *context)
{
  YY_USE (yyx0);
  YY_USE (yyx1);

#if CYPHER_YYDEBUG
  YY_FPRINTF ((stderr, "Ambiguity detected.\n"));
  YY_FPRINTF ((stderr, "Option 1,\n"));
  yyreportTree (yyx0, 2);
  YY_FPRINTF ((stderr, "\nOption 2,\n"));
  yyreportTree (yyx1, 2);
  YY_FPRINTF ((stderr, "\n"));
#endif

  yyerror (yylocp, context, YY_("syntax is ambiguous"));
  return yyabort;
}

/** Resolve the locations for each of the YYN1 states in *YYSTACKP,
 *  ending at YYS1.  Has no effect on previously resolved states.
 *  The first semantic option of a state is always chosen.  */
static void
yyresolveLocations (yyGLRState *yys1, int yyn1,
                    yyGLRStack *yystackp, cypher_parser_context *context)
{
  if (0 < yyn1)
    {
      yyresolveLocations (yys1->yypred, yyn1 - 1, yystackp, context);
      if (!yys1->yyresolved)
        {
          yyGLRStackItem yyrhsloc[1 + YYMAXRHS];
          int yynrhs;
          yySemanticOption *yyoption = yys1->yysemantics.yyfirstVal;
          YY_ASSERT (yyoption);
          yynrhs = yyrhsLength (yyoption->yyrule);
          if (0 < yynrhs)
            {
              yyGLRState *yys;
              int yyn;
              yyresolveLocations (yyoption->yystate, yynrhs,
                                  yystackp, context);
              for (yys = yyoption->yystate, yyn = yynrhs;
                   yyn > 0;
                   yys = yys->yypred, yyn -= 1)
                yyrhsloc[yyn].yystate.yyloc = yys->yyloc;
            }
          else
            {
              /* Both yyresolveAction and yyresolveLocations traverse the GSS
                 in reverse rightmost order.  It is only necessary to invoke
                 yyresolveLocations on a subforest for which yyresolveAction
                 would have been invoked next had an ambiguity not been
                 detected.  Thus the location of the previous state (but not
                 necessarily the previous state itself) is guaranteed to be
                 resolved already.  */
              yyGLRState *yyprevious = yyoption->yystate;
              yyrhsloc[0].yystate.yyloc = yyprevious->yyloc;
            }
          YYLLOC_DEFAULT ((yys1->yyloc), yyrhsloc, yynrhs);
        }
    }
}

/** Resolve the ambiguity represented in state YYS in *YYSTACKP,
 *  perform the indicated actions, and set the semantic value of YYS.
 *  If result != yyok, the chain of semantic options in YYS has been
 *  cleared instead or it has been left unmodified except that
 *  redundant options may have been removed.  Regardless of whether
 *  result = yyok, YYS has been left with consistent data so that
 *  yydestroyGLRState can be invoked if necessary.  */
static YYRESULTTAG
yyresolveValue (yyGLRState* yys, yyGLRStack* yystackp, cypher_parser_context *context)
{
  yySemanticOption* yyoptionList = yys->yysemantics.yyfirstVal;
  yySemanticOption* yybest = yyoptionList;
  yySemanticOption** yypp;
  yybool yymerge = yyfalse;
  YYSTYPE yyval;
  YYRESULTTAG yyflag;
  YYLTYPE *yylocp = &yys->yyloc;

  for (yypp = &yyoptionList->yynext; *yypp != YY_NULLPTR; )
    {
      yySemanticOption* yyp = *yypp;

      if (yyidenticalOptions (yybest, yyp))
        {
          yymergeOptionSets (yybest, yyp);
          *yypp = yyp->yynext;
        }
      else
        {
          switch (yypreference (yybest, yyp))
            {
            case 0:
              yyresolveLocations (yys, 1, yystackp, context);
              return yyreportAmbiguity (yybest, yyp, yylocp, context);
              break;
            case 1:
              yymerge = yytrue;
              break;
            case 2:
              break;
            case 3:
              yybest = yyp;
              yymerge = yyfalse;
              break;
            default:
              /* This cannot happen so it is not worth a YY_ASSERT (yyfalse),
                 but some compilers complain if the default case is
                 omitted.  */
              break;
            }
          yypp = &yyp->yynext;
        }
    }

  if (yymerge)
    {
      yySemanticOption* yyp;
      int yyprec = yydprec[yybest->yyrule];
      yyflag = yyresolveAction (yybest, yystackp, &yyval, yylocp, context);
      if (yyflag == yyok)
        for (yyp = yybest->yynext; yyp != YY_NULLPTR; yyp = yyp->yynext)
          {
            if (yyprec == yydprec[yyp->yyrule])
              {
                YYSTYPE yyval_other;
                YYLTYPE yydummy;
                yyflag = yyresolveAction (yyp, yystackp, &yyval_other, &yydummy, context);
                if (yyflag != yyok)
                  {
                    yydestruct ("Cleanup: discarding incompletely merged value for",
                                yy_accessing_symbol (yys->yylrState),
                                &yyval, yylocp, context);
                    break;
                  }
                yyuserMerge (yymerger[yyp->yyrule], &yyval, &yyval_other);
              }
          }
    }
  else
    yyflag = yyresolveAction (yybest, yystackp, &yyval, yylocp, context);

  if (yyflag == yyok)
    {
      yys->yyresolved = yytrue;
      yys->yysemantics.yyval = yyval;
    }
  else
    yys->yysemantics.yyfirstVal = YY_NULLPTR;
  return yyflag;
}

static YYRESULTTAG
yyresolveStack (yyGLRStack* yystackp, cypher_parser_context *context)
{
  if (yystackp->yysplitPoint != YY_NULLPTR)
    {
      yyGLRState* yys;
      int yyn;

      for (yyn = 0, yys = yystackp->yytops.yystates[0];
           yys != yystackp->yysplitPoint;
           yys = yys->yypred, yyn += 1)
        continue;
      YYCHK (yyresolveStates (yystackp->yytops.yystates[0], yyn, yystackp
                             , context));
    }
  return yyok;
}

/** Called when returning to deterministic operation to clean up the extra
 * stacks. */
static void
yycompressStack (yyGLRStack* yystackp)
{
  /* yyr is the state after the split point.  */
  yyGLRState *yyr;

  if (yystackp->yytops.yysize != 1 || yystackp->yysplitPoint == YY_NULLPTR)
    return;

  {
    yyGLRState *yyp, *yyq;
    for (yyp = yystackp->yytops.yystates[0], yyq = yyp->yypred, yyr = YY_NULLPTR;
         yyp != yystackp->yysplitPoint;
         yyr = yyp, yyp = yyq, yyq = yyp->yypred)
      yyp->yypred = yyr;
  }

  yystackp->yyspaceLeft += yystackp->yynextFree - yystackp->yyitems;
  yystackp->yynextFree = YY_REINTERPRET_CAST (yyGLRStackItem*, yystackp->yysplitPoint) + 1;
  yystackp->yyspaceLeft -= yystackp->yynextFree - yystackp->yyitems;
  yystackp->yysplitPoint = YY_NULLPTR;
  yystackp->yylastDeleted = YY_NULLPTR;

  while (yyr != YY_NULLPTR)
    {
      yystackp->yynextFree->yystate = *yyr;
      yyr = yyr->yypred;
      yystackp->yynextFree->yystate.yypred = &yystackp->yynextFree[-1].yystate;
      yystackp->yytops.yystates[0] = &yystackp->yynextFree->yystate;
      yystackp->yynextFree += 1;
      yystackp->yyspaceLeft -= 1;
    }
}

static YYRESULTTAG
yyprocessOneStack (yyGLRStack* yystackp, YYPTRDIFF_T yyk,
                   YYPTRDIFF_T yyposn, YYLTYPE *yylocp, cypher_parser_context *context)
{
  while (yystackp->yytops.yystates[yyk] != YY_NULLPTR)
    {
      yy_state_t yystate = yystackp->yytops.yystates[yyk]->yylrState;
      YY_DPRINTF ((stderr, "Stack %ld Entering state %d\n",
                   YY_CAST (long, yyk), yystate));

      YY_ASSERT (yystate != YYFINAL);

      if (yyisDefaultedState (yystate))
        {
          YYRESULTTAG yyflag;
          yyRuleNum yyrule = yydefaultAction (yystate);
          if (yyrule == 0)
            {
              YY_DPRINTF ((stderr, "Stack %ld dies.\n", YY_CAST (long, yyk)));
              yymarkStackDeleted (yystackp, yyk);
              return yyok;
            }
          yyflag = yyglrReduce (yystackp, yyk, yyrule, yyimmediate[yyrule], context);
          if (yyflag == yyerr)
            {
              YY_DPRINTF ((stderr,
                           "Stack %ld dies "
                           "(predicate failure or explicit user error).\n",
                           YY_CAST (long, yyk)));
              yymarkStackDeleted (yystackp, yyk);
              return yyok;
            }
          if (yyflag != yyok)
            return yyflag;
        }
      else
        {
          yysymbol_kind_t yytoken = yygetToken (&yychar, yystackp, context);
          const short* yyconflicts;
          const int yyaction = yygetLRActions (yystate, yytoken, &yyconflicts);
          yystackp->yytops.yylookaheadNeeds[yyk] = yytrue;

          for (/* nothing */; *yyconflicts; yyconflicts += 1)
            {
              YYRESULTTAG yyflag;
              YYPTRDIFF_T yynewStack = yysplitStack (yystackp, yyk);
              YY_DPRINTF ((stderr, "Splitting off stack %ld from %ld.\n",
                           YY_CAST (long, yynewStack), YY_CAST (long, yyk)));
              yyflag = yyglrReduce (yystackp, yynewStack,
                                    *yyconflicts,
                                    yyimmediate[*yyconflicts], context);
              if (yyflag == yyok)
                YYCHK (yyprocessOneStack (yystackp, yynewStack,
                                          yyposn, yylocp, context));
              else if (yyflag == yyerr)
                {
                  YY_DPRINTF ((stderr, "Stack %ld dies.\n", YY_CAST (long, yynewStack)));
                  yymarkStackDeleted (yystackp, yynewStack);
                }
              else
                return yyflag;
            }

          if (yyisShiftAction (yyaction))
            break;
          else if (yyisErrorAction (yyaction))
            {
              YY_DPRINTF ((stderr, "Stack %ld dies.\n", YY_CAST (long, yyk)));
              yymarkStackDeleted (yystackp, yyk);
              break;
            }
          else
            {
              YYRESULTTAG yyflag = yyglrReduce (yystackp, yyk, -yyaction,
                                                yyimmediate[-yyaction], context);
              if (yyflag == yyerr)
                {
                  YY_DPRINTF ((stderr,
                               "Stack %ld dies "
                               "(predicate failure or explicit user error).\n",
                               YY_CAST (long, yyk)));
                  yymarkStackDeleted (yystackp, yyk);
                  break;
                }
              else if (yyflag != yyok)
                return yyflag;
            }
        }
    }
  return yyok;
}

/* Put in YYARG at most YYARGN of the expected tokens given the
   current YYSTACKP, and return the number of tokens stored in YYARG.  If
   YYARG is null, return the number of expected tokens (guaranteed to
   be less than YYNTOKENS).  */
static int
yypcontext_expected_tokens (const yyGLRStack* yystackp,
                            yysymbol_kind_t yyarg[], int yyargn)
{
  /* Actual size of YYARG. */
  int yycount = 0;
  int yyn = yypact[yystackp->yytops.yystates[0]->yylrState];
  if (!yypact_value_is_default (yyn))
    {
      /* Start YYX at -YYN if negative to avoid negative indexes in
         YYCHECK.  In other words, skip the first -YYN actions for
         this state because they are default actions.  */
      int yyxbegin = yyn < 0 ? -yyn : 0;
      /* Stay within bounds of both yycheck and yytname.  */
      int yychecklim = YYLAST - yyn + 1;
      int yyxend = yychecklim < YYNTOKENS ? yychecklim : YYNTOKENS;
      int yyx;
      for (yyx = yyxbegin; yyx < yyxend; ++yyx)
        if (yycheck[yyx + yyn] == yyx && yyx != YYSYMBOL_YYerror
            && !yytable_value_is_error (yytable[yyx + yyn]))
          {
            if (!yyarg)
              ++yycount;
            else if (yycount == yyargn)
              return 0;
            else
              yyarg[yycount++] = YY_CAST (yysymbol_kind_t, yyx);
          }
    }
  if (yyarg && yycount == 0 && 0 < yyargn)
    yyarg[0] = YYSYMBOL_YYEMPTY;
  return yycount;
}

static int
yy_syntax_error_arguments (const yyGLRStack* yystackp,
                           yysymbol_kind_t yyarg[], int yyargn)
{
  yysymbol_kind_t yytoken = yychar == CYPHER_CYPHER_YYEMPTY ? YYSYMBOL_YYEMPTY : YYTRANSLATE (yychar);
  /* Actual size of YYARG. */
  int yycount = 0;
  /* There are many possibilities here to consider:
     - If this state is a consistent state with a default action, then
       the only way this function was invoked is if the default action
       is an error action.  In that case, don't check for expected
       tokens because there are none.
     - The only way there can be no lookahead present (in yychar) is if
       this state is a consistent state with a default action.  Thus,
       detecting the absence of a lookahead is sufficient to determine
       that there is no unexpected or expected token to report.  In that
       case, just report a simple "syntax error".
     - Don't assume there isn't a lookahead just because this state is a
       consistent state with a default action.  There might have been a
       previous inconsistent state, consistent state with a non-default
       action, or user semantic action that manipulated yychar.
     - Of course, the expected token list depends on states to have
       correct lookahead information, and it depends on the parser not
       to perform extra reductions after fetching a lookahead from the
       scanner and before detecting a syntax error.  Thus, state merging
       (from LALR or IELR) and default reductions corrupt the expected
       token list.  However, the list is correct for canonical LR with
       one exception: it will still contain any token that will not be
       accepted due to an error action in a later state.
  */
  if (yytoken != YYSYMBOL_YYEMPTY)
    {
      int yyn;
      if (yyarg)
        yyarg[yycount] = yytoken;
      ++yycount;
      yyn = yypcontext_expected_tokens (yystackp,
                                        yyarg ? yyarg + 1 : yyarg, yyargn - 1);
      if (yyn == YYENOMEM)
        return YYENOMEM;
      else
        yycount += yyn;
    }
  return yycount;
}



static void
yyreportSyntaxError (yyGLRStack* yystackp, cypher_parser_context *context)
{
  if (yystackp->yyerrState != 0)
    return;
  {
  yybool yysize_overflow = yyfalse;
  char* yymsg = YY_NULLPTR;
  enum { YYARGS_MAX = 5 };
  /* Internationalized format string. */
  const char *yyformat = YY_NULLPTR;
  /* Arguments of yyformat: reported tokens (one for the "unexpected",
     one per "expected"). */
  yysymbol_kind_t yyarg[YYARGS_MAX];
  /* Cumulated lengths of YYARG.  */
  YYPTRDIFF_T yysize = 0;

  /* Actual size of YYARG. */
  int yycount
    = yy_syntax_error_arguments (yystackp, yyarg, YYARGS_MAX);
  if (yycount == YYENOMEM)
    yyMemoryExhausted (yystackp);

  switch (yycount)
    {
#define YYCASE_(N, S)                   \
      case N:                           \
        yyformat = S;                   \
      break
    default: /* Avoid compiler warnings. */
      YYCASE_(0, YY_("syntax error"));
      YYCASE_(1, YY_("syntax error, unexpected %s"));
      YYCASE_(2, YY_("syntax error, unexpected %s, expecting %s"));
      YYCASE_(3, YY_("syntax error, unexpected %s, expecting %s or %s"));
      YYCASE_(4, YY_("syntax error, unexpected %s, expecting %s or %s or %s"));
      YYCASE_(5, YY_("syntax error, unexpected %s, expecting %s or %s or %s or %s"));
#undef YYCASE_
    }

  /* Compute error message size.  Don't count the "%s"s, but reserve
     room for the terminator.  */
  yysize = yystrlen (yyformat) - 2 * yycount + 1;
  {
    int yyi;
    for (yyi = 0; yyi < yycount; ++yyi)
      {
        YYPTRDIFF_T yysz
          = yystrlen (yysymbol_name (yyarg[yyi]));
        if (YYSIZE_MAXIMUM - yysize < yysz)
          yysize_overflow = yytrue;
        else
          yysize += yysz;
      }
  }

  if (!yysize_overflow)
    yymsg = YY_CAST (char *, YYMALLOC (YY_CAST (YYSIZE_T, yysize)));

  if (yymsg)
    {
      char *yyp = yymsg;
      int yyi = 0;
      while ((*yyp = *yyformat))
        {
          if (*yyp == '%' && yyformat[1] == 's' && yyi < yycount)
            {
              yyp = yystpcpy (yyp, yysymbol_name (yyarg[yyi++]));
              yyformat += 2;
            }
          else
            {
              ++yyp;
              ++yyformat;
            }
        }
      yyerror (&yylloc, context, yymsg);
      YYFREE (yymsg);
    }
  else
    {
      yyerror (&yylloc, context, YY_("syntax error"));
      yyMemoryExhausted (yystackp);
    }
  }
  yynerrs += 1;
}

/* Recover from a syntax error on *YYSTACKP, assuming that *YYSTACKP->YYTOKENP,
   yylval, and yylloc are the syntactic category, semantic value, and location
   of the lookahead.  */
static void
yyrecoverSyntaxError (yyGLRStack* yystackp, cypher_parser_context *context)
{
  if (yystackp->yyerrState == 3)
    /* We just shifted the error token and (perhaps) took some
       reductions.  Skip tokens until we can proceed.  */
    while (yytrue)
      {
        yysymbol_kind_t yytoken;
        int yyj;
        if (yychar == CYPHER_YYEOF)
          yyFail (yystackp, &yylloc, context, YY_NULLPTR);
        if (yychar != CYPHER_CYPHER_YYEMPTY)
          {
            /* We throw away the lookahead, but the error range
               of the shifted error token must take it into account.  */
            yyGLRState *yys = yystackp->yytops.yystates[0];
            yyGLRStackItem yyerror_range[3];
            yyerror_range[1].yystate.yyloc = yys->yyloc;
            yyerror_range[2].yystate.yyloc = yylloc;
            YYLLOC_DEFAULT ((yys->yyloc), yyerror_range, 2);
            yytoken = YYTRANSLATE (yychar);
            yydestruct ("Error: discarding",
                        yytoken, &yylval, &yylloc, context);
            yychar = CYPHER_CYPHER_YYEMPTY;
          }
        yytoken = yygetToken (&yychar, yystackp, context);
        yyj = yypact[yystackp->yytops.yystates[0]->yylrState];
        if (yypact_value_is_default (yyj))
          return;
        yyj += yytoken;
        if (yyj < 0 || YYLAST < yyj || yycheck[yyj] != yytoken)
          {
            if (yydefact[yystackp->yytops.yystates[0]->yylrState] != 0)
              return;
          }
        else if (! yytable_value_is_error (yytable[yyj]))
          return;
      }

  /* Reduce to one stack.  */
  {
    YYPTRDIFF_T yyk;
    for (yyk = 0; yyk < yystackp->yytops.yysize; yyk += 1)
      if (yystackp->yytops.yystates[yyk] != YY_NULLPTR)
        break;
    if (yyk >= yystackp->yytops.yysize)
      yyFail (yystackp, &yylloc, context, YY_NULLPTR);
    for (yyk += 1; yyk < yystackp->yytops.yysize; yyk += 1)
      yymarkStackDeleted (yystackp, yyk);
    yyremoveDeletes (yystackp);
    yycompressStack (yystackp);
  }

  /* Pop stack until we find a state that shifts the error token.  */
  yystackp->yyerrState = 3;
  while (yystackp->yytops.yystates[0] != YY_NULLPTR)
    {
      yyGLRState *yys = yystackp->yytops.yystates[0];
      int yyj = yypact[yys->yylrState];
      if (! yypact_value_is_default (yyj))
        {
          yyj += YYSYMBOL_YYerror;
          if (0 <= yyj && yyj <= YYLAST && yycheck[yyj] == YYSYMBOL_YYerror
              && yyisShiftAction (yytable[yyj]))
            {
              /* Shift the error token.  */
              int yyaction = yytable[yyj];
              /* First adjust its location.*/
              YYLTYPE yyerrloc;
              yystackp->yyerror_range[2].yystate.yyloc = yylloc;
              YYLLOC_DEFAULT (yyerrloc, (yystackp->yyerror_range), 2);
              YY_SYMBOL_PRINT ("Shifting", yy_accessing_symbol (yyaction),
                               &yylval, &yyerrloc);
              yyglrShift (yystackp, 0, yyaction,
                          yys->yyposn, &yylval, &yyerrloc);
              yys = yystackp->yytops.yystates[0];
              break;
            }
        }
      yystackp->yyerror_range[1].yystate.yyloc = yys->yyloc;
      if (yys->yypred != YY_NULLPTR)
        yydestroyGLRState ("Error: popping", yys, context);
      yystackp->yytops.yystates[0] = yys->yypred;
      yystackp->yynextFree -= 1;
      yystackp->yyspaceLeft += 1;
    }
  if (yystackp->yytops.yystates[0] == YY_NULLPTR)
    yyFail (yystackp, &yylloc, context, YY_NULLPTR);
}

#define YYCHK1(YYE)                             \
  do {                                          \
    switch (YYE) {                              \
    case yyok:     break;                       \
    case yyabort:  goto yyabortlab;             \
    case yyaccept: goto yyacceptlab;            \
    case yyerr:    goto yyuser_error;           \
    case yynomem:  goto yyexhaustedlab;         \
    default:       goto yybuglab;               \
    }                                           \
  } while (0)

/*----------.
| yyparse.  |
`----------*/

int
yyparse (cypher_parser_context *context)
{
  int yyresult;
  yyGLRStack yystack;
  yyGLRStack* const yystackp = &yystack;
  YYPTRDIFF_T yyposn;

  YY_DPRINTF ((stderr, "Starting parse\n"));

  yychar = CYPHER_CYPHER_YYEMPTY;
  yylval = yyval_default;
  yylloc = yyloc_default;

  if (! yyinitGLRStack (yystackp, YYINITDEPTH))
    goto yyexhaustedlab;
  switch (YYSETJMP (yystack.yyexception_buffer))
    {
    case 0: break;
    case 1: goto yyabortlab;
    case 2: goto yyexhaustedlab;
    default: goto yybuglab;
    }
  yyglrShift (&yystack, 0, 0, 0, &yylval, &yylloc);
  yyposn = 0;

  while (yytrue)
    {
      /* For efficiency, we have two loops, the first of which is
         specialized to deterministic operation (single stack, no
         potential ambiguity).  */
      /* Standard mode. */
      while (yytrue)
        {
          yy_state_t yystate = yystack.yytops.yystates[0]->yylrState;
          YY_DPRINTF ((stderr, "Entering state %d\n", yystate));
          if (yystate == YYFINAL)
            goto yyacceptlab;
          if (yyisDefaultedState (yystate))
            {
              yyRuleNum yyrule = yydefaultAction (yystate);
              if (yyrule == 0)
                {
                  yystack.yyerror_range[1].yystate.yyloc = yylloc;
                  yyreportSyntaxError (&yystack, context);
                  goto yyuser_error;
                }
              YYCHK1 (yyglrReduce (&yystack, 0, yyrule, yytrue, context));
            }
          else
            {
              yysymbol_kind_t yytoken = yygetToken (&yychar, yystackp, context);
              const short* yyconflicts;
              int yyaction = yygetLRActions (yystate, yytoken, &yyconflicts);
              if (*yyconflicts)
                /* Enter nondeterministic mode.  */
                break;
              if (yyisShiftAction (yyaction))
                {
                  YY_SYMBOL_PRINT ("Shifting", yytoken, &yylval, &yylloc);
                  yychar = CYPHER_CYPHER_YYEMPTY;
                  yyposn += 1;
                  yyglrShift (&yystack, 0, yyaction, yyposn, &yylval, &yylloc);
                  if (0 < yystack.yyerrState)
                    yystack.yyerrState -= 1;
                }
              else if (yyisErrorAction (yyaction))
                {
                  yystack.yyerror_range[1].yystate.yyloc = yylloc;
                  /* Issue an error message unless the scanner already
                     did. */
                  if (yychar != CYPHER_CYPHER_YYerror)
                    yyreportSyntaxError (&yystack, context);
                  goto yyuser_error;
                }
              else
                YYCHK1 (yyglrReduce (&yystack, 0, -yyaction, yytrue, context));
            }
        }

      /* Nondeterministic mode. */
      while (yytrue)
        {
          yysymbol_kind_t yytoken_to_shift;
          YYPTRDIFF_T yys;

          for (yys = 0; yys < yystack.yytops.yysize; yys += 1)
            yystackp->yytops.yylookaheadNeeds[yys] = yychar != CYPHER_CYPHER_YYEMPTY;

          /* yyprocessOneStack returns one of three things:

              - An error flag.  If the caller is yyprocessOneStack, it
                immediately returns as well.  When the caller is finally
                yyparse, it jumps to an error label via YYCHK1.

              - yyok, but yyprocessOneStack has invoked yymarkStackDeleted
                (&yystack, yys), which sets the top state of yys to NULL.  Thus,
                yyparse's following invocation of yyremoveDeletes will remove
                the stack.

              - yyok, when ready to shift a token.

             Except in the first case, yyparse will invoke yyremoveDeletes and
             then shift the next token onto all remaining stacks.  This
             synchronization of the shift (that is, after all preceding
             reductions on all stacks) helps prevent double destructor calls
             on yylval in the event of memory exhaustion.  */

          for (yys = 0; yys < yystack.yytops.yysize; yys += 1)
            YYCHK1 (yyprocessOneStack (&yystack, yys, yyposn, &yylloc, context));
          yyremoveDeletes (&yystack);
          if (yystack.yytops.yysize == 0)
            {
              yyundeleteLastStack (&yystack);
              if (yystack.yytops.yysize == 0)
                yyFail (&yystack, &yylloc, context, YY_("syntax error"));
              YYCHK1 (yyresolveStack (&yystack, context));
              YY_DPRINTF ((stderr, "Returning to deterministic operation.\n"));
              yystack.yyerror_range[1].yystate.yyloc = yylloc;
              yyreportSyntaxError (&yystack, context);
              goto yyuser_error;
            }

          /* If any yyglrShift call fails, it will fail after shifting.  Thus,
             a copy of yylval will already be on stack 0 in the event of a
             failure in the following loop.  Thus, yychar is set to CYPHER_CYPHER_YYEMPTY
             before the loop to make sure the user destructor for yylval isn't
             called twice.  */
          yytoken_to_shift = YYTRANSLATE (yychar);
          yychar = CYPHER_CYPHER_YYEMPTY;
          yyposn += 1;
          for (yys = 0; yys < yystack.yytops.yysize; yys += 1)
            {
              yy_state_t yystate = yystack.yytops.yystates[yys]->yylrState;
              const short* yyconflicts;
              int yyaction = yygetLRActions (yystate, yytoken_to_shift,
                              &yyconflicts);
              /* Note that yyconflicts were handled by yyprocessOneStack.  */
              YY_DPRINTF ((stderr, "On stack %ld, ", YY_CAST (long, yys)));
              YY_SYMBOL_PRINT ("shifting", yytoken_to_shift, &yylval, &yylloc);
              yyglrShift (&yystack, yys, yyaction, yyposn,
                          &yylval, &yylloc);
              YY_DPRINTF ((stderr, "Stack %ld now in state %d\n",
                           YY_CAST (long, yys),
                           yystack.yytops.yystates[yys]->yylrState));
            }

          if (yystack.yytops.yysize == 1)
            {
              YYCHK1 (yyresolveStack (&yystack, context));
              YY_DPRINTF ((stderr, "Returning to deterministic operation.\n"));
              yycompressStack (&yystack);
              break;
            }
        }
      continue;
    yyuser_error:
      yyrecoverSyntaxError (&yystack, context);
      yyposn = yystack.yytops.yystates[0]->yyposn;
    }

 yyacceptlab:
  yyresult = 0;
  goto yyreturnlab;

 yybuglab:
  YY_ASSERT (yyfalse);
  goto yyabortlab;

 yyabortlab:
  yyresult = 1;
  goto yyreturnlab;

 yyexhaustedlab:
  yyerror (&yylloc, context, YY_("memory exhausted"));
  yyresult = 2;
  goto yyreturnlab;

 yyreturnlab:
  if (yychar != CYPHER_CYPHER_YYEMPTY)
    yydestruct ("Cleanup: discarding lookahead",
                YYTRANSLATE (yychar), &yylval, &yylloc, context);

  /* If the stack is well-formed, pop the stack until it is empty,
     destroying its entries as we go.  But free the stack regardless
     of whether it is well-formed.  */
  if (yystack.yyitems)
    {
      yyGLRState** yystates = yystack.yytops.yystates;
      if (yystates)
        {
          YYPTRDIFF_T yysize = yystack.yytops.yysize;
          YYPTRDIFF_T yyk;
          for (yyk = 0; yyk < yysize; yyk += 1)
            if (yystates[yyk])
              {
                while (yystates[yyk])
                  {
                    yyGLRState *yys = yystates[yyk];
                    yystack.yyerror_range[1].yystate.yyloc = yys->yyloc;
                    if (yys->yypred != YY_NULLPTR)
                      yydestroyGLRState ("Cleanup: popping", yys, context);
                    yystates[yyk] = yys->yypred;
                    yystack.yynextFree -= 1;
                    yystack.yyspaceLeft += 1;
                  }
                break;
              }
        }
      yyfreeGLRStack (&yystack);
    }

  return yyresult;
}

/* DEBUGGING ONLY */
#if CYPHER_YYDEBUG
/* Print *YYS and its predecessors. */
static void
yy_yypstack (yyGLRState* yys)
{
  if (yys->yypred)
    {
      yy_yypstack (yys->yypred);
      YY_FPRINTF ((stderr, " -> "));
    }
  YY_FPRINTF ((stderr, "%d@%ld", yys->yylrState, YY_CAST (long, yys->yyposn)));
}

/* Print YYS (possibly NULL) and its predecessors. */
static void
yypstates (yyGLRState* yys)
{
  if (yys == YY_NULLPTR)
    YY_FPRINTF ((stderr, "<null>"));
  else
    yy_yypstack (yys);
  YY_FPRINTF ((stderr, "\n"));
}

/* Print the stack #YYK.  */
static void
yypstack (yyGLRStack* yystackp, YYPTRDIFF_T yyk)
{
  yypstates (yystackp->yytops.yystates[yyk]);
}

/* Print all the stacks.  */
static void
yypdumpstack (yyGLRStack* yystackp)
{
#define YYINDEX(YYX)                                                    \
  YY_CAST (long,                                                        \
           ((YYX)                                                       \
            ? YY_REINTERPRET_CAST (yyGLRStackItem*, (YYX)) - yystackp->yyitems \
            : -1))

  yyGLRStackItem* yyp;
  for (yyp = yystackp->yyitems; yyp < yystackp->yynextFree; yyp += 1)
    {
      YY_FPRINTF ((stderr, "%3ld. ",
                   YY_CAST (long, yyp - yystackp->yyitems)));
      if (*YY_REINTERPRET_CAST (yybool *, yyp))
        {
          YY_ASSERT (yyp->yystate.yyisState);
          YY_ASSERT (yyp->yyoption.yyisState);
          YY_FPRINTF ((stderr, "Res: %d, LR State: %d, posn: %ld, pred: %ld",
                       yyp->yystate.yyresolved, yyp->yystate.yylrState,
                       YY_CAST (long, yyp->yystate.yyposn),
                       YYINDEX (yyp->yystate.yypred)));
          if (! yyp->yystate.yyresolved)
            YY_FPRINTF ((stderr, ", firstVal: %ld",
                         YYINDEX (yyp->yystate.yysemantics.yyfirstVal)));
        }
      else
        {
          YY_ASSERT (!yyp->yystate.yyisState);
          YY_ASSERT (!yyp->yyoption.yyisState);
          YY_FPRINTF ((stderr, "Option. rule: %d, state: %ld, next: %ld",
                       yyp->yyoption.yyrule - 1,
                       YYINDEX (yyp->yyoption.yystate),
                       YYINDEX (yyp->yyoption.yynext)));
        }
      YY_FPRINTF ((stderr, "\n"));
    }

  YY_FPRINTF ((stderr, "Tops:"));
  {
    YYPTRDIFF_T yyi;
    for (yyi = 0; yyi < yystackp->yytops.yysize; yyi += 1)
      YY_FPRINTF ((stderr, "%ld: %ld; ", YY_CAST (long, yyi),
                   YYINDEX (yystackp->yytops.yystates[yyi])));
    YY_FPRINTF ((stderr, "\n"));
  }
#undef YYINDEX
}
#endif

#undef yylval
#undef yychar
#undef yynerrs
#undef yylloc

/* Substitute the variable and function names.  */
#define yyparse cypher_yyparse
#define yylex   cypher_yylex
#define yyerror cypher_yyerror
#define yylval  cypher_yylval
#define yychar  cypher_yychar
#define yydebug cypher_yydebug
#define yynerrs cypher_yynerrs
#define yylloc  cypher_yylloc


#line 2207 "src-tauri/crates/keeper-graph-query/vendor/graphqlite/src/backend/parser/cypher_gram.y"


/* Error handling function */
void cypher_yyerror(CYPHER_YYLTYPE *yylloc, cypher_parser_context *context, const char *msg)
{
    if (!context || !msg) {
        return;
    }

    /* If the scanner already raised a specific error (e.g. IntegerOverflow),
     * keep its message — Bison's generic 'syntax error, unexpected end of
     * file' would otherwise mask it. */
    if (context->has_error && context->error_message) {
        return;
    }

    context->has_error = true;
    context->error_location = yylloc ? yylloc->first_line : -1;
    context->error_column = yylloc ? yylloc->first_column : -1;

    /* Create error message with line number - Bison's detailed error mode
     * provides good context about what was expected */
    char error_buffer[512];
    /* Bison's own error is the openCypher SyntaxError UnexpectedSyntax; two
     * tokens say more: a parameter where a pattern takes only a map literal
     * (`MATCH (n $p)`, Match1 [6]), and a range without `*` in a
     * relationship pattern (`[:T 1..2]`, Match4 [9]). */
    const char *cls = "";
    if (strncmp(msg, "syntax error", 12) == 0) {
        if (strstr(msg, "unexpected PARAMETER, expecting ')'") || strstr(msg, "unexpected PARAMETER, expecting ']'"))
            cls = "SyntaxError: InvalidParameterUse: ";
        else if (strstr(msg, "unexpected DOT_DOT, expecting ']'"))
            cls = "SyntaxError: InvalidRelationshipPattern: ";
        else
            cls = "SyntaxError: UnexpectedSyntax: ";
    }
    if (yylloc && yylloc->first_line > 0) {
        if (yylloc->first_column > 0) {
            snprintf(error_buffer, sizeof(error_buffer),
                     "%sLine %d, Col %d: %s", cls, yylloc->first_line, yylloc->first_column, msg);
        } else {
            snprintf(error_buffer, sizeof(error_buffer),
                     "%sLine %d: %s", cls, yylloc->first_line, msg);
        }
    } else {
        snprintf(error_buffer, sizeof(error_buffer), "%s%s", cls, msg);
    }

    free(context->error_message);
    context->error_message = strdup(error_buffer);
}

/* cypher_yylex function is implemented in cypher_parser.c */
