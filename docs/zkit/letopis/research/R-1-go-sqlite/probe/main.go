// Throwaway integration probe; not Engram product code.
package main

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/binary"
	"fmt"
	"math"
	"math/rand/v2"
	"os"
	"runtime"
	"strconv"
	"time"

	"golang.org/x/sys/unix"
)

func main() {
	if err := run(context.Background()); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(ctx context.Context) error {
	if len(os.Args) != 3 {
		return fmt.Errorf("usage: probe DB_PATH VECTOR_COUNT")
	}
	n, err := strconv.Atoi(os.Args[2])
	if err != nil || n < 1 {
		return fmt.Errorf("invalid vector count %q", os.Args[2])
	}
	db, err := openDB(os.Args[1])
	if err != nil {
		return err
	}
	defer db.Close()
	db.SetMaxOpenConns(3)
	db.SetMaxIdleConns(3)
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
		for _, q := range []string{"SELECT sqlite_version()", "SELECT vec1_info()", "PRAGMA journal_mode", "PRAGMA synchronous", "PRAGMA fullfsync", "PRAGMA foreign_keys"} {
			var value any
			if err := c.QueryRowContext(ctx, q).Scan(&value); err != nil {
				return fmt.Errorf("connection %d %s: %w", i, q, err)
			}
			fmt.Printf("config conn=%d query=%q value=%v\n", i, q, value)
		}
	}
	if _, err := a.ExecContext(ctx, `
		CREATE TABLE nodes(id INTEGER PRIMARY KEY, selected INTEGER);
		CREATE VIRTUAL TABLE ft USING fts5(text, content='', contentless_delete=1);
		CREATE VIRTUAL TABLE v USING vec1(vector);
		INSERT INTO nodes VALUES(1,0),(2,1),(3,0);
		INSERT INTO ft(rowid,text) VALUES(1,'persistent memory');
		INSERT INTO v(rowid,vector) VALUES(1,vec1_from_json('[1,0]')),(2,vec1_from_json('[0,1]')),(3,vec1_from_json('[-1,0]'));
		INSERT INTO v(cmd,arg) VALUES('rebuild','{"index":"flat","distance":"cos"}');
	`); err != nil {
		return fmt.Errorf("setup: %w", err)
	}
	var text any
	var id int
	if err := a.QueryRowContext(ctx, "SELECT rowid,text FROM ft WHERE ft MATCH 'memory'").Scan(&id, &text); err != nil || id != 1 || text != nil {
		return fmt.Errorf("contentless FTS: id=%d text=%v error=%v", id, text, err)
	}
	fmt.Println("check contentless_fts=PASS")
	if _, err := a.ExecContext(ctx, "DELETE FROM ft WHERE rowid=1"); err != nil {
		return err
	}
	if err := a.QueryRowContext(ctx, "SELECT count(*) FROM ft WHERE ft MATCH 'memory'").Scan(&id); err != nil || id != 0 {
		return fmt.Errorf("FTS delete: count=%d error=%v", id, err)
	}
	fmt.Println("check contentless_delete=PASS")
	q := vector([]float32{1, 0})
	for _, c := range []*sql.Conn{a, b} {
		if err := c.QueryRowContext(ctx, "SELECT rowid FROM v(?, '{\"K\":1}')", q).Scan(&id); err != nil || id != 1 {
			return fmt.Errorf("nearest: id=%d error=%v", id, err)
		}
	}
	fmt.Println("check extension_two_connections=PASS")
	tx, err := b.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	if _, err := tx.ExecContext(ctx, "UPDATE v SET vector=vec1_from_json('[0,-1]') WHERE rowid=1"); err != nil {
		tx.Rollback()
		return err
	}
	if err := tx.Rollback(); err != nil {
		return err
	}
	if err := b.QueryRowContext(ctx, "SELECT rowid FROM v(?, '{\"K\":1}')", q).Scan(&id); err != nil || id != 1 {
		return fmt.Errorf("rollback: id=%d error=%v", id, err)
	}
	fmt.Println("check vector_update_rollback=PASS")
	read, err := a.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		return err
	}
	defer read.Rollback()
	var before, after []byte
	if err := read.QueryRowContext(ctx, "SELECT vector FROM v WHERE rowid=1").Scan(&before); err != nil {
		return err
	}
	if _, err := b.ExecContext(ctx, "UPDATE v SET vector=vec1_from_json('[0,-1]') WHERE rowid=1"); err != nil {
		return fmt.Errorf("writer with pinned reader: %w", err)
	}
	if err := read.QueryRowContext(ctx, "SELECT vector FROM v WHERE rowid=1").Scan(&after); err != nil || !bytes.Equal(before, after) {
		return fmt.Errorf("pinned vector snapshot error=%v old=%x new=%x", err, before, after)
	}
	if err := read.QueryRowContext(ctx, "SELECT rowid FROM v(?, '{\"K\":1}')", q).Scan(&id); err != nil || id != 1 {
		return fmt.Errorf("pinned nearest: id=%d error=%v", id, err)
	}
	if err := b.QueryRowContext(ctx, "SELECT rowid FROM v(?, '{\"K\":1}')", q).Scan(&id); err != nil || id != 2 {
		return fmt.Errorf("new nearest: id=%d error=%v", id, err)
	}
	if err := read.Commit(); err != nil {
		return err
	}
	fmt.Println("check wal_vector_snapshot=PASS")
	if _, err := b.ExecContext(ctx, "UPDATE v SET vector=vec1_from_json('[1,0]') WHERE rowid=1"); err != nil {
		return err
	}
	for _, params := range []string{`{"K":1}`, `{"K":1,"streaming":1}`} {
		var count int
		err := b.QueryRowContext(ctx, "SELECT count(*) FROM (SELECT vv.rowid FROM v(?, ?) vv JOIN nodes n ON n.id=vv.rowid WHERE n.selected=1 LIMIT 1)", q, params).Scan(&count)
		if err != nil {
			return fmt.Errorf("filtered query: %w", err)
		}
		fmt.Printf("filter params=%s count=%d\n", params, count)
	}
	if _, err := b.ExecContext(ctx, "DELETE FROM v WHERE rowid=3"); err != nil {
		return err
	}
	if err := b.QueryRowContext(ctx, "SELECT count(*) FROM v").Scan(&id); err != nil || id != 2 {
		return fmt.Errorf("vector delete: count=%d error=%v", id, err)
	}
	fmt.Println("check vector_delete=PASS")
	if err := a.Close(); err != nil {
		return err
	}
	if err := b.Close(); err != nil {
		return err
	}
	db.SetMaxIdleConns(0)
	if err := db.QueryRowContext(ctx, "SELECT vec1_info()").Scan(&text); err != nil {
		return fmt.Errorf("connection recreation: %w", err)
	}
	fmt.Println("check extension_reopened_connection=PASS")
	db.SetMaxIdleConns(3)
	const dim = 384
	rng := rand.New(rand.NewPCG(1, 2))
	start := time.Now()
	tx, err = db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if _, err := tx.ExecContext(ctx, "CREATE VIRTUAL TABLE big USING vec1(vector)"); err != nil {
		return err
	}
	stmt, err := tx.PrepareContext(ctx, "INSERT INTO big(rowid,vector) VALUES(?,?)")
	if err != nil {
		return err
	}
	for i := range n {
		values := make([]float32, dim)
		for j := range values {
			values[j] = rng.Float32()*2 - 1
		}
		if _, err := stmt.ExecContext(ctx, i+1, vector(values)); err != nil {
			stmt.Close()
			return err
		}
	}
	if err := stmt.Close(); err != nil {
		return err
	}
	if err := tx.Commit(); err != nil {
		return err
	}
	fmt.Printf("load vectors=%d dim=%d elapsed=%s\n", n, dim, time.Since(start))
	start = time.Now()
	if _, err := db.ExecContext(ctx, `INSERT INTO big(cmd,arg) VALUES('rebuild','{"index":"flat","distance":"cos"}')`); err != nil {
		return fmt.Errorf("flat rebuild: %w", err)
	}
	fmt.Printf("rebuild elapsed=%s\n", time.Since(start))
	values := make([]float32, dim)
	for i := range values {
		values[i] = rng.Float32()*2 - 1
	}
	query := vector(values)
	for i := range 5 {
		start = time.Now()
		rows, err := db.QueryContext(ctx, `SELECT rowid FROM big(?, '{"K":10}')`, query)
		if err != nil {
			return err
		}
		count := 0
		for rows.Next() {
			if err := rows.Scan(&id); err != nil {
				rows.Close()
				return err
			}
			count++
		}
		err = rows.Err()
		rows.Close()
		if err != nil || count != 10 {
			return fmt.Errorf("big query count=%d error=%v", count, err)
		}
		fmt.Printf("search iteration=%d rows=%d elapsed=%s\n", i, count, time.Since(start))
	}
	for _, querySQL := range []string{`WITH RECURSIVE seq(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM seq WHERE x<100000000) SELECT sum(x) FROM seq`, `SELECT rowid FROM big(?, '{"K":10}')`} {
		args := []any{}
		if querySQL[0] == 'S' {
			args = append(args, query)
		}
		deadline, cancel := context.WithTimeout(ctx, time.Millisecond)
		start = time.Now()
		err := db.QueryRowContext(deadline, querySQL, args...).Scan(&text)
		fmt.Printf("cancel query=%q elapsed=%s error=%v context=%v\n", querySQL, time.Since(start), err, deadline.Err())
		cancel()
	}
	if err := db.QueryRowContext(ctx, "SELECT 1").Scan(&id); err != nil || id != 1 {
		return fmt.Errorf("connection after cancellation: value=%d error=%v", id, err)
	}
	fmt.Println("check connection_after_cancel=PASS")
	deadline, cancel := context.WithTimeout(ctx, time.Millisecond)
	start = time.Now()
	_, rebuildErr := db.ExecContext(deadline, `INSERT INTO big(cmd,arg) VALUES('rebuild','{"index":"flat","distance":"cos"}')`)
	fmt.Printf("cancel rebuild elapsed=%s error=%v context=%v\n", time.Since(start), rebuildErr, deadline.Err())
	cancel()
	if err := db.QueryRowContext(ctx, "SELECT count(*) FROM big").Scan(&id); err != nil || id != n {
		return fmt.Errorf("after cancelled rebuild count=%d error=%v", id, err)
	}
	if err := db.QueryRowContext(ctx, `SELECT rowid FROM big(?, '{"K":1}')`, query).Scan(&id); err != nil {
		return fmt.Errorf("search after cancelled rebuild: %w", err)
	}
	fmt.Println("check index_after_cancelled_rebuild=PASS")
	var usage unix.Rusage
	if err := unix.Getrusage(unix.RUSAGE_SELF, &usage); err != nil {
		return err
	}
	var stats runtime.MemStats
	runtime.ReadMemStats(&stats)
	fmt.Printf("memory maxrss_raw=%d GOOS=%s go_heap_sys=%d\n", usage.Maxrss, runtime.GOOS, stats.HeapSys)
	return nil
}

func vector(values []float32) []byte {
	buf := make([]byte, 4*len(values))
	for i, value := range values {
		binary.LittleEndian.PutUint32(buf[4*i:], math.Float32bits(value))
	}
	return buf
}
