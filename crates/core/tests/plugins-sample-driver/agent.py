#!/usr/bin/env python3
"""数镜驱动插件 fixture:最小 JSON-RPC over stdio 实现(内存假数据)。"""
import sys, json

TABLES = [
    {"name": "users", "kind": "table", "comment": "用户"},
    {"name": "orders", "kind": "table", "comment": ""},
]
COLS = [
    {"name": "id", "type": "INTEGER", "nullable": False, "pk": True, "default": "", "comment": ""},
    {"name": "name", "type": "TEXT", "nullable": True, "pk": False, "default": "", "comment": "姓名"},
]
ROWS = [["1", "张三"], ["2", "李四"]]


def respond(method, params):
    if method == "ping":
        return {"ok": True}
    if method == "connect":
        return {"version": "SampleDB 1.0 (fixture agent)"}
    if method == "databases":
        return {"databases": ["main"]}
    if method == "tables":
        return {"tables": TABLES}
    if method == "structure":
        return {"columns": COLS, "pk": ["id"]}
    if method == "count":
        return {"count": 2}
    if method == "execute_batch":
        sqls = params.get("sqls") or []
        return {"affected": len(sqls)}
    if method == "query":
        sql = str(params.get("sql", ""))
        up = sql.strip().upper()
        if "COUNT" in up:
            return {"columns": ["count"], "rows": [["2"]], "affected": 0, "truncated": False}
        if up.startswith("DELETE") or up.startswith("UPDATE") or up.startswith("INSERT"):
            return {"columns": [], "rows": [], "affected": 1, "truncated": False}
        return {"columns": ["id", "name"], "rows": ROWS, "affected": 0, "truncated": False}
    return {"error": {"message": f"未知方法: {method}"}}


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
            rid = req.get("id", 0)
            method = req.get("method", "")
            params = req.get("params") or {}
            out = {"jsonrpc": "2.0", "id": rid, "result": respond(method, params)}
        except Exception as e:  # noqa
            rid = 0
            try:
                bad = json.loads(line)
                rid = bad.get("id", 0)
            except Exception:
                pass
            out = {"jsonrpc": "2.0", "id": rid, "error": {"message": str(e)}}
        sys.stdout.write(json.dumps(out, ensure_ascii=False) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
