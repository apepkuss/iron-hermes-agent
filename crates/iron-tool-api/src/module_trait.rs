use crate::registry::ToolRegistry;

/// 统一的工具模块注册接口。
pub trait ToolModule {
    fn register(self: Box<Self>, registry: &mut ToolRegistry);
}
