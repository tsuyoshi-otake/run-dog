# 調査ピン

- date: 2026-09-05
- OS: Windows 10.0.26200
- Rust: rustc 1.97.1 (8bab26f4f 2026-07-14)
- `git fetch origin` 実施後
- 作業開始時 `origin/main` / HEAD: `a8e870c0f1cad7cf4e431f6d563c8670c5626868`
- tag: `v1.1.21`
- 基準として渡された古い SHA も同じ値。再 fetch しても `origin/main` は移動していなかった。
- 既存 Issue / PR / コメントは正解として扱わない。本ディレクトリの分類は現行 tree の静的読取とローカル再現に基づく。
- 旧ピン `2c0494c`（v1.1.20）上の証跡は現 main の証明に使わない。

## コマンド

```
git fetch origin
git rev-parse origin/main
```

結果: `a8e870c0f1cad7cf4e431f6d563c8670c5626868`
