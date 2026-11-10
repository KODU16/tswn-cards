# tswn-cards 引擎维护约定

`KODU16/tswn-cards` 是 `namerena-cards` 的固定战斗引擎源码仓库。以后仅在本仓库的
`main` 分支修改 cards 专用战斗规则，**不再向 `KODU16/tswn-core` 主仓库提交 cards 功能**。

## 发布链路

1. 在本仓库修改 Rust / WASM 绑定代码并提交到 `main`。
2. 执行 `python scripts/build_wasm.py --release --clean`，产物位于
   `crates/tswn_wasm/dist/wasm/pkg/`。
3. 将 `pkg/` 的 `tswn_wasm.js`、`tswn_wasm_bg.wasm`、`tswn_wasm.d.ts` 同步至
   `namerena-cards/vendor/tswn_wasm_duel/`。
4. 在 cards 仓库执行 `node tools/verify-engine.mjs vendor/tswn_wasm_duel`、
   `node tools/verify-relics.mjs` 与 `npm run build:site`；三者通过后才可提交 cards。

## 当前迁移状态

此仓库从已验证可编译的 `namerena-card-duel` 源码初始化，固定了 cards 遗物的新增运行时
逻辑。当前 cards 已提交的 vendor WASM 仍含一组未能从该源码基线复现的强制行动与能力卡
扩展，因此**不得仅凭构建成功替换生产 vendor 产物**。

已向上游请求提供该 vendor 对应的完整源码提交（包含 `runtime/forced_action.rs` 及相关
扩展）。收到准确基线后，应先把差异迁入本仓库，再完成一次完整的 cards 验证并更新 vendor。

## 版本固定

cards 发布记录必须写入本仓库的完整提交 SHA；更新引擎时，以该 SHA 作为唯一源码来源。
这样主仓库的后续提交不会改变 cards 的构建结果。
