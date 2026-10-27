//! Role-based access control: which roles may perform which actions. Deny by
//! default. *Which rows* a permitted action reaches — own records, assigned
//! offerings, a department — is enforced separately by Postgres row-level
//! security.

use std::{fmt, str::FromStr};

/// The roles of architecture.md §7.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Student,
    Faculty,
    Hod,
    ExamCell,
    Accounts,
    SupportAgent,
    Admin,
    /// Break-glass: everything, and every request is audited and alerted.
    SuperAdmin,
    /// The vendor's operator (Acadex is sold to universities): runs the
    /// deployment and onboards the university, and sees none of its records.
    PlatformAdmin,
}

impl Role {
    pub const ALL: [Self; 9] = [
        Self::Student,
        Self::Faculty,
        Self::Hod,
        Self::ExamCell,
        Self::Accounts,
        Self::SupportAgent,
        Self::Admin,
        Self::SuperAdmin,
        Self::PlatformAdmin,
    ];

    /// Matches `identity.user_role` in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Student => "student",
            Self::Faculty => "faculty",
            Self::Hod => "hod",
            Self::ExamCell => "exam_cell",
            Self::Accounts => "accounts",
            Self::SupportAgent => "support_agent",
            Self::Admin => "admin",
            Self::SuperAdmin => "super_admin",
            Self::PlatformAdmin => "platform_admin",
        }
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|role| role.as_str() == value)
            .ok_or_else(|| format!("unknown role {value:?}"))
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

macro_rules! permissions {
    ($($variant:ident => $name:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Permission { $($variant),* }

        impl Permission {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// `resource:action`, as reported to the frontend.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $name),* }
            }
        }
    };
}

permissions! {
    AttendanceRead => "attendance:read",
    AttendanceMark => "attendance:mark",
    // Subjects per semester and who teaches them.
    CurriculumManage => "curriculum:manage",
    // Term-end promotion and graduation of a program's students.
    PromotionManage => "promotion:manage",
    // Department- or institution-wide shortage lists.
    AttendanceShortages => "attendance:shortages",
    // Operations: dead letters, the audit chain, the outbox (the platform's).
    OpsManage => "ops:manage",
    // The university's profile and administrators (the platform's).
    PlatformManage => "platform:manage",
    StudentsRead => "students:read",
    AuditReadAll => "audit:read_all",
    NoticesRead => "notices:read",
    NoticesPublish => "notices:publish",
    ResultsRead => "results:read",
    ResultsPublish => "results:publish",
    MarksEnter => "marks:enter",
    FeesRead => "fees:read",
    FeesPay => "fees:pay",
    // The accounts office: fee structures, concessions, ledgers, refunds, reconciliation.
    FeesManage => "fees:manage",
    UsersManage => "users:manage",
    SettingsManage => "settings:manage",
    CalendarManage => "calendar:manage",
    CorrectionsRequest => "corrections:request",
    CorrectionsDecide => "corrections:decide",
    MoocEnrol => "mooc:enrol",
    MoocReview => "mooc:review",
    MoocManage => "mooc:manage",
    ExamsManage => "exams:manage",
    ExamsRegister => "exams:register",
    ActivitiesSubmit => "activities:submit",
    ActivitiesReview => "activities:review",
    ProfileEdit => "profile:edit",
    ProfileReview => "profile:review",
    SupportRaise => "support:raise",
    SupportHandle => "support:handle",
}

/// The permission matrix. Anything not listed is denied.
pub fn allows(role: Role, permission: Permission) -> bool {
    use Permission::*;
    // Everyone at the university can ask its helpdesk for help.
    if permission == SupportRaise {
        return role != Role::PlatformAdmin;
    }
    match role {
        // Running the deployment is the platform's, not the university's.
        Role::SuperAdmin => !matches!(permission, OpsManage | PlatformManage),
        Role::PlatformAdmin => matches!(permission, OpsManage | PlatformManage),
        Role::Student => matches!(
            permission,
            AttendanceRead
                | NoticesRead
                | ResultsRead
                | FeesRead
                | FeesPay
                | CorrectionsRequest
                | MoocEnrol
                | ExamsRegister
                | ActivitiesSubmit
                | ProfileEdit
        ),
        Role::Faculty => matches!(
            permission,
            AttendanceRead
                | AttendanceMark
                | StudentsRead
                | NoticesRead
                | MarksEnter
                | ActivitiesReview
        ),
        Role::Hod => matches!(
            permission,
            AttendanceRead
                | AttendanceShortages
                | CurriculumManage
                | PromotionManage
                | StudentsRead
                | NoticesRead
                | ResultsRead
                | CorrectionsDecide
                | MoocReview
                | MoocManage
                | ActivitiesReview
        ),
        Role::ExamCell => matches!(
            permission,
            AttendanceRead
                | AttendanceShortages
                | StudentsRead
                | NoticesRead
                | NoticesPublish
                | ResultsRead
                | ResultsPublish
                | MarksEnter
                | CalendarManage
                | ExamsManage
        ),
        Role::Accounts => matches!(permission, StudentsRead | NoticesRead | FeesManage),
        Role::SupportAgent => matches!(permission, StudentsRead | NoticesRead | SupportHandle),
        Role::Admin => matches!(
            permission,
            StudentsRead
                | AttendanceShortages
                | CurriculumManage
                | PromotionManage
                | AuditReadAll
                | NoticesRead
                | NoticesPublish
                | UsersManage
                | SettingsManage
                | CalendarManage
                | MoocManage
                | ProfileReview
                | SupportHandle
        ),
    }
}

pub fn permissions(role: Role) -> impl Iterator<Item = Permission> {
    Permission::ALL
        .iter()
        .copied()
        .filter(move |&permission| allows(role, permission))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole matrix as a grid, so any change is a visible diff here.
    /// Columns follow `Role::ALL`: student, faculty, hod, exam_cell,
    /// accounts, support_agent, admin, super_admin, platform_admin.
    #[test]
    fn matrix() {
        let grid = [
            ("attendance:read", "XXXX...X."),
            ("attendance:mark", ".X.....X."),
            ("students:read", ".XXXXXXX."),
            ("audit:read_all", "......XX."),
            ("notices:read", "XXXXXXXX."),
            ("notices:publish", "...X..XX."),
            ("results:read", "X.XX...X."),
            ("results:publish", "...X...X."),
            ("marks:enter", ".X.X...X."),
            ("fees:read", "X......X."),
            ("fees:pay", "X......X."),
            ("fees:manage", "....X..X."),
            ("attendance:shortages", "..XX..XX."),
            ("curriculum:manage", "..X...XX."),
            ("promotion:manage", "..X...XX."),
            ("ops:manage", "........X"),
            ("platform:manage", "........X"),
            ("users:manage", "......XX."),
            ("settings:manage", "......XX."),
            ("calendar:manage", "...X..XX."),
            ("corrections:request", "X......X."),
            ("corrections:decide", "..X....X."),
            ("mooc:enrol", "X......X."),
            ("mooc:review", "..X....X."),
            ("mooc:manage", "..X...XX."),
            ("exams:manage", "...X...X."),
            ("exams:register", "X......X."),
            ("activities:submit", "X......X."),
            ("activities:review", ".XX....X."),
            ("profile:edit", "X......X."),
            ("profile:review", "......XX."),
            ("support:raise", "XXXXXXXX."),
            ("support:handle", ".....XXX."),
        ];
        assert_eq!(
            grid.len(),
            Permission::ALL.len(),
            "every permission has a row"
        );
        for (name, row) in grid {
            let permission = Permission::ALL
                .iter()
                .copied()
                .find(|p| p.as_str() == name)
                .unwrap_or_else(|| panic!("unknown permission {name}"));
            for (role, cell) in Role::ALL.into_iter().zip(row.chars()) {
                assert_eq!(allows(role, permission), cell == 'X', "{role} {name}");
            }
        }
    }

    #[test]
    fn roles_round_trip_through_their_database_names() {
        for role in Role::ALL {
            assert_eq!(role.as_str().parse::<Role>(), Ok(role));
        }
        assert!("root".parse::<Role>().is_err());
    }
}
