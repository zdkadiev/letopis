//go:build !native

package main

import (
	"database/sql"
	"net/url"

	"github.com/ncruces/go-sqlite3"
	"github.com/ncruces/go-sqlite3/driver"
	"github.com/ncruces/go-sqlite3/ext/fts5"
	"github.com/ncruces/go-sqlite3/ext/vec1"
)

func openDB(path string) (*sql.DB, error) {
	u := url.URL{Scheme: "file", Path: path}
	q := u.Query()
	for _, pragma := range []string{"busy_timeout(5000)", "journal_mode(WAL)", "synchronous(FULL)", "fullfsync(ON)", "foreign_keys(ON)"} {
		q.Add("_pragma", pragma)
	}
	u.RawQuery = q.Encode()
	return driver.Open(u.String(), func(c *sqlite3.Conn) error {
		if err := fts5.Register(c); err != nil {
			return err
		}
		return vec1.Register(c)
	})
}
