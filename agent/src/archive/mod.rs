//! 压缩/解压能力探测
//!
//! Agent 不内置压缩算法，解压依赖服务器系统工具。本模块通过内嵌极小样本
//! 真实执行「列表 + 解压」来回答该机器实际支持哪些格式，而不是仅判断命令是否存在。
//!
//! ⚠️ 探测与执行使用同一份 PATH（Worker 继承 Manager 环境），因此探测结果
//! 与实际解压能力一致；暂不支持 PATH 之外的自定义工具目录。

pub mod commands;
pub mod probe;
#[cfg(unix)]
pub mod validate;
