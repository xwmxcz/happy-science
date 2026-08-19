//! Registry for research capabilities that mission contracts may require.
//!
//! Mission prompts and runtime deployment both consume this table so a renamed
//! bundled skill cannot silently leave a mission asking for a nonexistent tool.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResearchCapability {
    LiteratureSurvey,
    TraceabilityReview,
    IntegrityAudit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilitySpec {
    pub id: &'static str,
    pub skill_name: &'static str,
    pub purpose: &'static str,
}

pub const ALL: &[ResearchCapability] = &[
    ResearchCapability::LiteratureSurvey,
    ResearchCapability::TraceabilityReview,
    ResearchCapability::IntegrityAudit,
];

impl ResearchCapability {
    pub const fn spec(self) -> CapabilitySpec {
        match self {
            Self::LiteratureSurvey => CapabilitySpec {
                id: "literature-survey",
                skill_name: "literature-survey",
                purpose: "search, classify, and synthesize literature from verified sources",
            },
            Self::TraceabilityReview => CapabilitySpec {
                id: "traceability-review",
                skill_name: "traceability-review",
                purpose: "resolve citations and trace claims, numbers, and figures to evidence",
            },
            Self::IntegrityAudit => CapabilitySpec {
                id: "integrity-audit",
                skill_name: "integrity-auditor",
                purpose: "audit image, numerical, and logical integrity with reviewable findings",
            },
        }
    }
}

pub fn missing_skill_manifests(skills_dir: &Path) -> Vec<&'static str> {
    ALL.iter()
        .map(|capability| capability.spec().skill_name)
        .filter(|name| !skills_dir.join(name).join("SKILL.md").is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_capability_has_a_bundled_manifest() {
        let repository_skills = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("runtime")
            .join("skills");
        for capability in ALL {
            let name = capability.spec().skill_name;
            let candidates = [
                repository_skills.join("core").join(name).join("SKILL.md"),
                repository_skills
                    .join("external")
                    .join("ai4s-skills")
                    .join(name)
                    .join("SKILL.md"),
            ];
            assert!(
                candidates.iter().any(|path| path.is_file()),
                "registered capability {name} has no bundled SKILL.md"
            );
        }
    }

    #[test]
    fn deployed_profile_audit_reports_only_missing_registered_skills() {
        let root =
            std::env::temp_dir().join(format!("happy-science-capabilities-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for capability in ALL {
            let name = capability.spec().skill_name;
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), "---\n").unwrap();
        }
        assert!(missing_skill_manifests(&root).is_empty());
        std::fs::remove_file(root.join("literature-survey/SKILL.md")).unwrap();
        assert_eq!(missing_skill_manifests(&root), vec!["literature-survey"]);
        let _ = std::fs::remove_dir_all(root);
    }
}
