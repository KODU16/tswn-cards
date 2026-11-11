# tswn-cards 引擎维护约定

`KODU16/tswn-cards` 是 `namerena-cards` 的固定战斗引擎源码仓库。
在本地 `cards-main` 分支维护，发布到该仓库的 `main`；cards 专用规则不推送到 `KODU16/tswn-core`。

## 发布链路

1. 修改 Rust / WASM 源码，执行 `cargo +nightly fmt --check` 和受影响 crate 的测试。
2. 执行 `cargo build -p tswn_wasm --target wasm32-unknown-unknown --release`。
3. 使用匹配依赖版本的 `wasm-bindgen --target web` 生成 JS、标准 WASM 及类型声明，同步到 `namerena-cards/vendor/tswn_wasm_duel/`。
4. 执行 cards 的引擎、遗物、能力、分身和进度回归，再运行 `npm run build:site`。
5. 记录引擎完整提交 SHA，推送两个项目的发布分支；核对远端提交以及线上静态资源的内容哈希。

## 已完成的移植

遗物包括开场状态、持续回复和护盾、低伤害拦截、原角色独立属性加成、负面状态附加狂暴、状态持续延长、分身衰减减免、负面状态返还、阵亡吞噬与背刺增强。

归档补丁中缺失的 `runtime/forced_action.rs` 已从本地保留的完整源码恢复。会话支持双方受控、开局逐角色 HP、负 MP 普攻、独立能力乘区和交错会话隔离；新产物必须保留这些现有接口。

## 版本固定

cards 的 `BUILD.md` 记录引擎完整 SHA 和补丁基线。固定 SHA 是发布的源码来源，构建成功后仍需实际运行 cards 回归。
