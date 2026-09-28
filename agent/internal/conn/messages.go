package conn

import agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"

// 消息的分类，对应 protocol.md「消息一览」。协议里加了消息，这里要同步改。

// isStableAgentMessage 判断 Agent 发的消息是不是稳定消息：版本不一致时也能发。
func isStableAgentMessage(msg *agentv1.AgentMessage) bool {
	switch msg.GetBody().(type) {
	case *agentv1.AgentMessage_Hello,
		*agentv1.AgentMessage_UpgradeResult,
		*agentv1.AgentMessage_UninstallResult,
		*agentv1.AgentMessage_TrafficReport,
		*agentv1.AgentMessage_ErrorReply:
		return true
	}
	return false
}

// isStableMasterMessage 判断主控发的消息是不是稳定消息：版本不一致时也要处理。
func isStableMasterMessage(msg *agentv1.MasterMessage) bool {
	switch msg.GetBody().(type) {
	case *agentv1.MasterMessage_HelloResult,
		*agentv1.MasterMessage_Upgrade,
		*agentv1.MasterMessage_Uninstall,
		*agentv1.MasterMessage_ErrorReply:
		return true
	}
	return false
}

// isMasterRequest 判断主控发的消息是不是需要回复的请求。
func isMasterRequest(msg *agentv1.MasterMessage) bool {
	switch msg.GetBody().(type) {
	case *agentv1.MasterMessage_Upgrade,
		*agentv1.MasterMessage_Uninstall,
		*agentv1.MasterMessage_CheckRealityTargets,
		*agentv1.MasterMessage_ScanRealityTargets:
		return true
	}
	return false
}
