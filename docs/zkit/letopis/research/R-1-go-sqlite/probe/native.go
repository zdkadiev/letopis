//go:build native

package main

import (
	"database/sql"
	"net/url"
	"os"

	sqlite3 "github.com/mattn/go-sqlite3"
)

func openDB(path string) (*sql.DB, error) {
	extensionPath := os.Getenv("ENGRAM_PROBE_EXTENSION")
	if extensionPath == "" {
		extensionPath = "/private/tmp/engram-go-sqlite-research/vec1.dylib"
	}
	sql.Register("engram-native", &sqlite3.SQLiteDriver{
		ConnectHook: func(c *sqlite3.SQLiteConn) error {
			if err := c.LoadExtension(extensionPath, "sqlite3_extension_init"); err != nil {
				return err
			}
			_, err := c.Exec("PRAGMA fullfsync=ON", nil)
			return err
		},
	})
	u := url.URL{Scheme: "file", Path: path}
	q := u.Query()
	q.Set("_journal_mode", "WAL")
	q.Set("_synchronous", "FULL")
	q.Set("_busy_timeout", "5000")
	q.Set("_foreign_keys", "on")
	u.RawQuery = q.Encode()
	return sql.Open("engram-native", u.String())
}
