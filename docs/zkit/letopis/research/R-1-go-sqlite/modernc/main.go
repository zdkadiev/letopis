// Throwaway compatibility probe for the alternative sqlite-vec extension.
package main

import (
	"context"
	"database/sql"
	"fmt"
	"os"

	_ "modernc.org/sqlite"
	_ "modernc.org/sqlite/vec"
)

func main() {
	if err := run(context.Background()); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(ctx context.Context) error {
	db, err := sql.Open("sqlite", "file:/private/tmp/engram-go-sqlite-research/modernc.db?_pragma=journal_mode(WAL)&_pragma=synchronous(FULL)&_pragma=fullfsync(ON)")
	if err != nil {
		return err
	}
	defer db.Close()
	db.SetMaxOpenConns(2)
	a, err := db.Conn(ctx)
	if err != nil {
		return err
	}
	defer a.Close()
	b, err := db.Conn(ctx)
	if err != nil {
		return err
	}
	defer b.Close()
	for i, c := range []*sql.Conn{a, b} {
		for _, query := range []string{"SELECT sqlite_version()", "SELECT vec_version()", "PRAGMA journal_mode", "PRAGMA synchronous", "PRAGMA fullfsync"} {
			var value any
			if err := c.QueryRowContext(ctx, query).Scan(&value); err != nil {
				return err
			}
			fmt.Printf("config conn=%d query=%q value=%v\n", i, query, value)
		}
	}
	if _, err := a.ExecContext(ctx, `CREATE VIRTUAL TABLE f USING fts5(text, content='', contentless_delete=1); INSERT INTO f(rowid,text) VALUES(1,'agent memory'); CREATE VIRTUAL TABLE v USING vec0(embedding float[2]); INSERT INTO v(rowid,embedding) VALUES(1,'[1,0]'),(2,'[0,1]');`); err != nil {
		return err
	}
	var id int
	var text any
	if err := a.QueryRowContext(ctx, "SELECT rowid,text FROM f WHERE f MATCH 'memory'").Scan(&id, &text); err != nil || id != 1 || text != nil {
		return fmt.Errorf("FTS id=%d text=%v error=%v", id, text, err)
	}
	if err := b.QueryRowContext(ctx, "SELECT rowid FROM v WHERE embedding MATCH '[1,0]' AND k=1 ORDER BY distance").Scan(&id); err != nil || id != 1 {
		return fmt.Errorf("vec0 id=%d error=%v", id, err)
	}
	fmt.Println("check modernc_fts5_vec0_two_connections=PASS")
	return nil
}
