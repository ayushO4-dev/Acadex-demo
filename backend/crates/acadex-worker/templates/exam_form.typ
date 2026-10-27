// Exam form acknowledgement: proof of registration for an exam session.
#let d = json(bytes(sys.inputs.data))

#set document(title: "Exam form " + d.form_id, author: d.institution)
#set page(paper: "a4", margin: (x: 2.2cm, y: 2cm))
#set text(font: "Libertinus Serif", size: 10.5pt)

#align(center)[
  #text(size: 17pt, weight: "bold", d.institution)
  #v(-0.4em)
  #text(size: 11pt, tracking: 2pt, fill: luma(80))[EXAMINATION FORM — ACKNOWLEDGEMENT]
]
#line(length: 100%, stroke: 0.6pt)

#grid(
  columns: (1fr, 1fr),
  row-gutter: 0.75em,
  [*Session* #h(0.4em) #d.session], align(right)[*Form* #h(0.4em) #d.kind],
  [*Student* #h(0.4em) #d.student_name], align(right)[*Roll no.* #h(0.4em) #d.roll_no],
  [*Program* #h(0.4em) #d.program], align(right)[*Semester* #h(0.4em) #d.semester],
)

#v(1em)
#table(
  columns: (auto, auto, 1fr, auto, auto),
  stroke: (x, y) => if y == 0 { (bottom: 0.6pt) } else { (bottom: 0.3pt + luma(200)) },
  inset: (x: 6pt, y: 7pt),
  table.header([*No.*], [*Code*], [*Subject*], [*Date*], [*Time*]),
  ..d.subjects.enumerate().map(((i, s)) => (str(i + 1), s.code, s.name, s.date, s.time)).flatten(),
)

#v(0.5em)
*Fee paid:* #d.fee #h(1fr) *Submitted:* #d.submitted_at

#v(1em)
#text(size: 9.5pt)[
  Keep this acknowledgement. Your hall ticket, with your seat, is issued before the examinations
  if you remain eligible (attendance and fees). Check the timetable for any changes.
]

#v(1fr)
#line(length: 100%, stroke: 0.3pt + luma(180))
#text(size: 8.5pt, fill: luma(90))[
  Computer-generated acknowledgement; no signature needed. Form reference #raw(d.form_id).
]
