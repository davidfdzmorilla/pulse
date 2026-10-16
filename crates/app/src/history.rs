//! Snapshot-based undo/redo.
//!
//! The app compares the project against the last committed snapshot once per
//! frame. A difference becomes an undo step as soon as no pointer button is held,
//! so a whole knob drag or note drag is a single step.

use crate::project::Project;

const MAX_STEPS: usize = 200;

pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    baseline: Project,
}

impl History {
    pub fn new(project: &Project) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            baseline: project.clone(),
        }
    }

    /// Forgets all history, e.g. after opening a file.
    pub fn reset(&mut self, project: &Project) {
        *self = Self::new(project);
    }

    /// Records a step if the project changed (and isn't being dragged); true then.
    pub fn commit(&mut self, project: &Project, interacting: bool) -> bool {
        if interacting || *project == self.baseline {
            return false;
        }
        let previous = std::mem::replace(&mut self.baseline, project.clone());
        self.undo.push(previous);
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, project: &mut Project) -> bool {
        let Some(previous) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(project, previous));
        self.baseline = project.clone();
        true
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(project, next));
        self.baseline = project.clone();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_are_undone_and_redone_in_order() {
        let mut project = Project::empty();
        let mut history = History::new(&project);
        project.tempo = 100.0;
        history.commit(&project, false);
        project.tempo = 90.0;
        history.commit(&project, true); // still dragging: no step yet
        history.commit(&project, false);

        assert!(history.undo(&mut project));
        assert_eq!(project.tempo, 100.0);
        assert!(history.undo(&mut project));
        assert_eq!(project.tempo, 120.0);
        assert!(!history.undo(&mut project));
        assert!(history.redo(&mut project));
        assert_eq!(project.tempo, 100.0);
    }
}
