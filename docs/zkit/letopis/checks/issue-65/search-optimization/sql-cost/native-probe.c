// Scoped SQL-only measurement; this is not a product implementation.
#include "sqlite3.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static void check(sqlite3 *db, int status) {
    if (status != SQLITE_OK) { fprintf(stderr, "%s\n", sqlite3_errmsg(db)); exit(1); }
}
static sqlite3_stmt *prepare(sqlite3 *db, const char *sql) {
    sqlite3_stmt *s = NULL; check(db, sqlite3_prepare_v2(db, sql, -1, &s, NULL)); return s;
}
static double now(void) {
    struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9;
}
static void measure(sqlite3 *db, const char *label, const char *sql, char **ids, int n, int cached) {
    sqlite3_stmt *s = cached ? prepare(db, sql) : NULL;
    long rows = 0, scans = 0; double start = now();
    for (int i = 0; i < n; ++i) {
        if (!cached) s = prepare(db, sql);
        if (sqlite3_bind_parameter_count(s)) {
            if (strcmp(label,"revision")==0) check(db, sqlite3_bind_int64(s,1,1));
            else check(db, sqlite3_bind_text(s, 1, ids[i], -1, SQLITE_STATIC));
            if (sqlite3_bind_parameter_count(s) == 2) check(db, sqlite3_bind_text(s, 2, ids[i], -1, SQLITE_STATIC));
        }
        int result;
        while ((result = sqlite3_step(s)) == SQLITE_ROW) {
            for (int c = 0; c < sqlite3_column_count(s); ++c) (void)sqlite3_column_text(s,c);
            ++rows;
        }
        if (result != SQLITE_DONE) { check(db,result); }
        scans += sqlite3_stmt_status(s, SQLITE_STMTSTATUS_FULLSCAN_STEP, 1);
        if (cached) { check(db, sqlite3_reset(s)); check(db, sqlite3_clear_bindings(s)); }
        else sqlite3_finalize(s);
    }
    if (cached) sqlite3_finalize(s);
    printf("timing\t%s\t%d\t%d\t%ld\t%ld\t%.9f\n", label,cached,n,rows,scans,now()-start);
}
static void plan(sqlite3 *db,const char *label,const char *sql) {
    char *query=sqlite3_mprintf("explain query plan %s",sql); sqlite3_stmt *s=prepare(db,query); sqlite3_free(query);
    while (sqlite3_step(s)==SQLITE_ROW) printf("plan\t%s\t%s\n",label,sqlite3_column_text(s,3)); sqlite3_finalize(s);
}
int main(int argc, char **argv) {
    if (argc!=2) return 2;
    sqlite3 *db=NULL;check(db,sqlite3_open_v2(argv[1],&db,SQLITE_OPEN_READONLY,NULL));
    check(db,sqlite3_exec(db,"begin",NULL,NULL,NULL));
    printf("sqlite\t%s\n",sqlite3_libversion());
    const char *node="select id, uri, kind, hash, created_at, source_snapshot, line_start, line_end, search_hash from nodes where uri=?1";
    const char *revision="select revision from project where id=?1";
    const char *edges="select source.uri as source_uri, target.uri as target_uri, edges.type as type from edges inner join nodes as source on source.id=edges.source_id inner join nodes as target on target.id=edges.target_id where source.uri=?1 union all select source.uri as source_uri, unresolved_edges.target_uri as target_uri, unresolved_edges.type as type from unresolved_edges inner join nodes as source on source.id=unresolved_edges.source_id where source.uri=?2 order by source_uri, target_uri, type";
    char **ids=malloc(sizeof(char*)*200000);int n=0;
    sqlite3_stmt *s=prepare(db,"select uri from nodes where kind<>'source' order by uri");
    while(sqlite3_step(s)==SQLITE_ROW) { ids[n++]=strdup((const char*)sqlite3_column_text(s,0)); }
    sqlite3_finalize(s);printf("knowledge\t%d\n",n);
    s=prepare(db,"select count(*) from text_index where text_index match '\"case\" AND \"000000\"'");
    if(sqlite3_step(s)!=SQLITE_ROW) return 3; printf("fts_matches\t%d\n",sqlite3_column_int(s,0));sqlite3_finalize(s);
    plan(db,"nodes",node);plan(db,"revision",revision);plan(db,"edges",edges);
    for(int repeat=0;repeat<3;++repeat) for(int cached=0;cached<2;++cached) {
        measure(db,"nodes",node,ids,n,cached); measure(db,"revision",revision,ids,n,cached); measure(db,"edges",edges,ids,n,cached);
    }
    check(db,sqlite3_exec(db,"rollback",NULL,NULL,NULL));sqlite3_close(db);
    for(int i=0;i<n;++i) free(ids[i]);free(ids);return 0;
}
