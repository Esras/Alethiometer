# Scion Hub fixtures

Hand-written from the Go JSON tags of GoogleCloudPlatform/scion at commit
`2220c46de0f66248044911428d20d5aafe242fe4` (2026-10-02), not recorded from a
live Hub. Used by `src/agents/scion` tests.

| File | Endpoint | Go type |
|---|---|---|
| `me.json` | `GET /api/v1/auth/me` | `hub.UserResponse` (`pkg/hub/handlers_auth.go`) |
| `projects.json` | `GET /api/v1/projects` | `hub.ListProjectsResponse` of `ProjectWithCapabilities` → `store.Project` (`pkg/hub/handlers_projects_core.go`, `pkg/store/models.go`) |
| `agents.json` | `GET /api/v1/projects/{id}/agents` | `hub.ListAgentsResponse` of `AgentWithCapabilities` → `store.Agent`, `store.AgentAppliedConfig` (`pkg/hub/handlers_agents_core.go`, `pkg/hub/response_types.go`) |
| `conversations.json` | `GET /api/v1/conversations?include_project_groups=` | `hub.conversationListResponse` → `store.Conversation` (`pkg/hub/handlers_conversations.go`) |
| `conversation.json` | `GET /api/v1/conversations/{id}` | `hub.conversationResponse` with `store.ConversationParticipant` |
| `room_messages.json`, `dm_messages.json` | `GET /api/v1/conversations/{id}/messages` | `store.ListResult[store.Message]` (newest first) |
| `interagent.json` | `GET /api/v1/chat/conversations/{dm key}/interagent` | `hub.interagentResponse` (`pkg/hub/handlers_chat_v2.go`) |
| `chat_send.json` | `POST /api/v1/chat/conversations/{topic}/messages` | `hub.chatMessageResponse` |
| `agent_message.json` | `POST /api/v1/agents/{id}/message` | `hub.MessageDeliveryResponse` (`pkg/hub/handlers_agent_messaging.go`) |
| `agent_not_running.json` | the same, agent stopped (409) | `hub.ErrorResponse` / `APIError` (`pkg/hub/errors.go`) |
| `templates.json` | `GET /api/v1/templates` | `hub.ListTemplatesResponse` → `store.Template` (`pkg/hub/template_handlers.go`) |
| `template_file.json` | `GET /api/v1/templates/{id}/files/agents.md` | `hub.TemplateFileContentResponse` (`pkg/hub/template_file_handlers.go`) |
| `harness_configs.json` | `GET /api/v1/harness-configs` | `hub.ListHarnessConfigsResponse` → `store.HarnessConfig` (`pkg/hub/harness_config_handlers.go`) |
| `providers.json` | `GET /api/v1/projects/{id}/providers` | `{"providers": [projectProviderView]}` → `store.ProjectProvider` (`pkg/hub/handlers_env_secrets.go`) |
| `create_agent.json` | `POST /api/v1/agents` | `hub.CreateAgentResponse` |
| `participant.json` | `POST /api/v1/conversations/{id}/participants` | `store.ConversationParticipant` |

Message `type` values are the closed enum in `pkg/messages/types.go`;
`dispatchState` is `pending | dispatched | failed | deferred`
(`store.MessageDispatch*`), and an unreachable primary is `failed` with
`dispatchFailureReason` "Agent unreachable (<phase>)" (`handlers_chat_v2.go`).
`store.Message` carries no metadata, so a system message's category isn't
visible here. The system sender `system:hub` is a guess.
