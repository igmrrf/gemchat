use std::collections::HashMap;

/// A Skill defines a set of specialized instructions and tools that can be
/// added to an agent.
pub trait Skill: Send + Sync {
    /// Unique name for the skill (e.g. "rust_expert")
    fn name(&self) -> &str;
    /// Specialized system prompt instructions to add to the agent
    fn instructions(&self) -> &str;
    /// Tools that this skill enables for the agent
    fn provided_tools(&self) -> Vec<String>;
}

/// A registry that manages all available skills.
pub struct SkillRegistry {
    skills: HashMap<String, Box<dyn Skill>>,
}

impl SkillRegistry {
    /// Create a new registry and register all built-in skills.
    pub fn new() -> Self {
        let mut reg = Self {
            skills: HashMap::new(),
        };
        reg.register(Box::new(RustExpert));
        reg.register(Box::new(TestDrivenDevelopment));
        reg.register(Box::new(WebExpert));
        reg
    }

    /// Add a new skill to the registry.
    pub fn register(&mut self, skill: Box<dyn Skill>) {
        self.skills.insert(skill.name().to_string(), skill);
    }

    /// Get a skill by name.
    pub fn get(&self, name: &str) -> Option<&dyn Skill> {
        self.skills.get(name).map(|s| s.as_ref())
    }

    /// List all registered skill names.
    pub fn list(&self) -> Vec<String> {
        self.skills.keys().cloned().collect()
    }
}

// --- Built-in Skills ---

/// Expertise in Rust development.
pub struct RustExpert;
impl Skill for RustExpert {
    fn name(&self) -> &str { "rust_expert" }
    fn instructions(&self) -> &str {
        "You are a Rust expert. Use idiomatic Rust patterns (e.g. results, options, traits). \
         Prefer using 'cargo check', 'cargo fmt', and 'cargo clippy' via the run_command tool \
         to ensure code quality. Follow the latest Rust edition conventions."
    }
    fn provided_tools(&self) -> Vec<String> {
        vec!["run_command".into(), "run_tests".into()]
    }
}

/// Priority on Test-Driven Development.
pub struct TestDrivenDevelopment;
impl Skill for TestDrivenDevelopment {
    fn name(&self) -> &str { "tdd" }
    fn instructions(&self) -> &str {
        "Always follow Test-Driven Development (TDD) principles. Before writing any implementation code, \
         write a failing test case that defines the expected behavior. Ensure the test fails, then \
         write the minimum code needed to make it pass."
    }
    fn provided_tools(&self) -> Vec<String> {
        vec!["run_tests".into()]
    }
}

/// Expertise in modern web development.
pub struct WebExpert;
impl Skill for WebExpert {
    fn name(&self) -> &str { "web_expert" }
    fn instructions(&self) -> &str {
        "You are a web development expert. Focus on performance, accessibility (A11y), and \
         modern CSS patterns like container queries and CSS variables. \
         When working with React/Next.js, prioritize Server Components and clean architecture."
    }
    fn provided_tools(&self) -> Vec<String> {
        vec!["run_command".into()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_registration() {
        let reg = SkillRegistry::new();
        assert!(reg.get("rust_expert").is_some());
        assert!(reg.get("tdd").is_some());
        assert!(reg.get("web_expert").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_skill_instructions() {
        let rust = RustExpert;
        assert!(rust.instructions().contains("Rust expert"));
    }
}
