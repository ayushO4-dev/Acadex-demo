// Grade card: one student's result for one semester, each subject at its
// latest published result. Data in sys.inputs.data (JSON, formatted by the
// worker).
#let d = json(bytes(sys.inputs.data))

#set document(title: "Grade card " + d.roll_no + " semester " + d.semester, author: d.institution)
#set page(paper: "a4", margin: (x: 2cm, y: 1.8cm))
#set text(font: "Libertinus Serif", size: 10.5pt)

#align(center)[
  #text(size: 17pt, weight: "bold", d.institution) \
  #text(size: 11pt, tracking: 2pt, fill: luma(80))[GRADE CARD] \
  #text(size: 10pt)[Semester #d.semester · academic year #d.academic_year]
]
#line(length: 100%, stroke: 0.6pt)

#grid(
  columns: (auto, 1fr, auto, auto),
  row-gutter: 0.7em,
  column-gutter: 0.8em,
  [*Name*], d.student_name, [*Roll no.*], d.roll_no,
  [*Program*], d.program, [*Issued*], d.issued_on,
)

#v(0.8em)
#table(
  columns: (auto, 1fr, auto, auto, auto, auto, auto, auto),
  align: (left, left, right, right, right, center, right, left),
  stroke: (x, y) => if y == 0 { (bottom: 0.6pt) } else { (bottom: 0.3pt + luma(200)) },
  inset: (x: 5pt, y: 6pt),
  table.header([*Code*], [*Subject*], [*Credits*], [*Internal*], [*External*], [*Grade*], [*Points*], [*Result*]),
  ..d.subjects.map(s => (
    s.code, s.name, s.credits, s.internal, s.external,
    text(weight: "bold", s.grade), s.points,
    if s.note == "" { s.result } else { s.result + super(s.note) },
  )).flatten(),
)

#v(0.6em)
#grid(
  columns: (1fr, 1fr, 1fr, 1fr),
  column-gutter: 0.8em,
  ..(("SGPA", d.sgpa), ("CGPA", d.cgpa), ("Credits earned", d.credits), ("Result", d.result)).map(((label, value)) =>
    box(width: 100%, stroke: 0.6pt + luma(160), inset: 8pt, radius: 3pt)[
      #text(size: 8.5pt, fill: luma(90), label) \
      #text(size: 14pt, weight: "bold", value)
    ]
  ),
)

#if d.notes.len() > 0 [
  #v(0.6em)
  #text(size: 9pt, fill: luma(60))[#d.notes.join(" · ")]
]

#v(0.8em)
#text(size: 9pt)[
  Total is internal plus external. Passing needs a pass grade and the minimum in the external exam.
  SGPA and CGPA count each subject's latest result.
]

#v(1fr)
#grid(
  columns: (1fr, 1fr),
  align(left)[#text(size: 8.5pt, fill: luma(90))[Computer-generated; no signature required. \ Reference #raw(d.reference).]],
  align(right)[#line(length: 5cm, stroke: 0.4pt) #text(size: 9pt)[Controller of Examinations]],
)
