// The demo's people: real accounts in the seed data, one per role that the
// core journey needs. Signing in as one mints a token for `demo|<email>`, which
// the seed links to that account (seeds/99_demo_identities.sql).

export type Persona = { email: string; name: string; role: string; story: string };

export const PERSONAS: Persona[] = [
  {
    email: "student.a@northgate.example",
    name: "Atharva Joshi",
    role: "Student",
    story: "Attendance above 75%, fees partly paid. Pay a fee and get the receipt.",
  },
  {
    email: "student.b@northgate.example",
    name: "Sanika Deshpande",
    role: "Student",
    story: "In attendance shortage, with a backlog. See the warnings.",
  },
  {
    email: "faculty.ece@northgate.example",
    name: "Dr. Revati Gokhale",
    role: "Faculty",
    story: "Marks today's attendance and enters internal marks.",
  },
  {
    email: "examcell@northgate.example",
    name: "Exam Cell Office",
    role: "Exam cell",
    story: "Semester 4 results are staged: request publishing. Posts notices.",
  },
  {
    email: "coe@northgate.example",
    name: "Dr. Vikram Bhosale",
    role: "Controller of Examinations",
    story: "Approves the exam cell's request. Publishing takes two people.",
  },
  {
    email: "registrar@northgate.example",
    name: "Dr. Anand Bapat",
    role: "Registrar",
    story: "Reads the audit log of everything everyone did.",
  },
];
