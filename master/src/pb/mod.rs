//! protobuf 生成的代码（在 master/ 下执行 buf generate，见 buf.gen.yaml），不要手改。

#[allow(clippy::all, clippy::pedantic, dead_code)]
pub mod openproxy {
    pub mod agent {
        pub mod v1 {
            include!("openproxy/agent/v1/openproxy.agent.v1.rs");
        }
    }
}

pub use openproxy::agent::v1 as agentv1;
