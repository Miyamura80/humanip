//! Route-config commands: validate a `routes.yaml`, export its JSON Schema,
//! and parse proxy usernames.

use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use humanip_core::policy::{parse_routes_yaml, ResolvedRisk, RoutesFile};
use humanip_core::username::{self, ProxyUser};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Default)]
pub struct ValidateRoutes;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ValidateRoutesInput {
    /// Contents of a routes.yaml file.
    pub yaml: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ValidatedRoute {
    pub name: String,
    pub sticky_minutes: u32,
    pub limits: ResolvedRisk,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ValidateRoutesOutput {
    pub routes: Vec<ValidatedRoute>,
}

#[async_trait]
impl Command for ValidateRoutes {
    type Input = ValidateRoutesInput;
    type Output = ValidateRoutesOutput;

    fn name(&self) -> &'static str {
        "validate_routes"
    }

    fn description(&self) -> &'static str {
        "Validate a routes.yaml and show the limits each route's risk dial resolves to."
    }

    async fn run(
        &self,
        input: ValidateRoutesInput,
        _cx: &Ctx<'_>,
    ) -> Result<ValidateRoutesOutput, CommandError> {
        let file = parse_routes_yaml(&input.yaml)
            .map_err(|e| CommandError::InvalidInput(e.to_string()))?;
        Ok(ValidateRoutesOutput {
            routes: file
                .routes
                .iter()
                .map(|r| ValidatedRoute {
                    name: r.name.clone(),
                    sticky_minutes: r.required_sticky_minutes(),
                    limits: r.resolved_risk(),
                })
                .collect(),
        })
    }
}

register_command!(ValidateRoutes);

#[derive(Default)]
pub struct RouteSchema;

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct RouteSchemaInput {}

#[async_trait]
impl Command for RouteSchema {
    type Input = RouteSchemaInput;
    type Output = Value;

    fn name(&self) -> &'static str {
        "route_schema"
    }

    fn description(&self) -> &'static str {
        "JSON Schema for routes.yaml (for editor autocompletion and CI validation)."
    }

    async fn run(&self, _input: RouteSchemaInput, _cx: &Ctx<'_>) -> Result<Value, CommandError> {
        serde_json::to_value(schemars::schema_for!(RoutesFile))
            .map_err(|e| CommandError::Other(e.to_string()))
    }
}

register_command!(RouteSchema);

#[derive(Default)]
pub struct ParseProxyUser;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ParseProxyUserInput {
    /// e.g. "user-route_agent-browser-session_r42-risk_40".
    pub username: String,
}

#[async_trait]
impl Command for ParseProxyUser {
    type Input = ParseProxyUserInput;
    type Output = ProxyUser;

    fn name(&self) -> &'static str {
        "parse_proxy_user"
    }

    fn description(&self) -> &'static str {
        "Parse per-connection settings (route, session, risk, geo, exit, target) from a proxy username."
    }

    async fn run(
        &self,
        input: ParseProxyUserInput,
        _cx: &Ctx<'_>,
    ) -> Result<ProxyUser, CommandError> {
        username::parse(&input.username).map_err(|e| CommandError::InvalidInput(e.to_string()))
    }
}

register_command!(ParseProxyUser);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AppContext;

    #[tokio::test]
    async fn validates_the_example_file() {
        let app = AppContext::default();
        let yaml = include_str!("../../../../examples/routes.yaml").to_string();
        let out = ValidateRoutes
            .run(ValidateRoutesInput { yaml }, &Ctx::new(&app))
            .await
            .unwrap();
        let names: Vec<_> = out.routes.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["mautrix-whatsapp", "agent-browser", "price-scrape"]);
    }

    #[tokio::test]
    async fn schema_names_the_route_fields() {
        let app = AppContext::default();
        let schema = RouteSchema
            .run(RouteSchemaInput {}, &Ctx::new(&app))
            .await
            .unwrap()
            .to_string();
        for field in [
            "on_pin_loss",
            "reliability_floor",
            "exit_types",
            "spend_cap_usd",
        ] {
            assert!(schema.contains(field), "schema is missing {field}");
        }
    }

    #[tokio::test]
    async fn bad_username_is_invalid_input() {
        let app = AppContext::default();
        let r = ParseProxyUser
            .run(
                ParseProxyUserInput {
                    username: "nope".into(),
                },
                &Ctx::new(&app),
            )
            .await;
        assert!(matches!(r, Err(CommandError::InvalidInput(_))));
    }
}
