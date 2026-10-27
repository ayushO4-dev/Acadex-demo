// Hall ticket. Data in sys.inputs.data (JSON, formatted by the worker); the QR
// code as SVG bytes in sys.inputs.qr; a photo, when there is one, in
// sys.inputs.photo.
#let d = json(bytes(sys.inputs.data))

#set document(title: "Hall ticket " + d.roll_no, author: d.institution)
#set page(paper: "a4", margin: (x: 2cm, y: 1.8cm))
#set text(font: "Libertinus Serif", size: 10.5pt)

#grid(
  columns: (1fr, auto),
  align(left)[
    #text(size: 17pt, weight: "bold", d.institution) \
    #text(size: 11pt, tracking: 2pt, fill: luma(80))[HALL TICKET] \
    #text(size: 10pt)[#d.session]
  ],
  align(right)[
    #image(sys.inputs.qr, width: 2.6cm)
    #text(size: 7.5pt, fill: luma(90))[Scan to verify]
  ],
)
#line(length: 100%, stroke: 0.6pt)

#grid(
  columns: (3cm, 1fr),
  column-gutter: 1em,
  if "photo" in sys.inputs {
    image(sys.inputs.photo, width: 3cm, height: 3.6cm, fit: "cover")
  } else {
    box(width: 3cm, height: 3.6cm, stroke: 0.6pt + luma(150), inset: 4pt)[
      #align(center + horizon)[#text(size: 20pt, fill: luma(120), d.initials) \ #text(size: 7pt, fill: luma(120))[Photo on file at the department]]
    ]
  },
  grid(
    columns: (auto, 1fr),
    row-gutter: 0.7em,
    column-gutter: 0.8em,
    [*Name*], d.student_name,
    [*Roll no.*], d.roll_no,
    [*Program*], d.program,
    [*Semester*], d.semester,
    [*Venue*], [#text(size: 12pt, weight: "bold", d.venue)],
    [*Seat*], [#text(size: 12pt, weight: "bold", d.seat)],
  ),
)

#v(0.8em)
#table(
  columns: (auto, auto, auto, 1fr, 3cm),
  stroke: (x, y) => if y == 0 { (bottom: 0.6pt) } else { (bottom: 0.3pt + luma(200)) },
  inset: (x: 5pt, y: 7pt),
  table.header([*Date*], [*Time*], [*Code*], [*Subject*], [*Invigilator*]),
  ..d.papers.map(p => (p.date, p.time, p.code, p.name, [])).flatten(),
)

#v(0.8em)
#text(size: 9.5pt)[
  *Instructions*
  #list(
    [Bring this hall ticket and your college ID card to every paper.],
    [Be seated 15 minutes before the start; no entry 30 minutes after it.],
    [Phones, smart watches and notes are not allowed in the hall.],
    [The invigilator signs against each paper you write.],
  )
]

#v(1fr)
#line(length: 100%, stroke: 0.3pt + luma(180))
#text(size: 8.5pt, fill: luma(90))[
  Valid only with the institute's online verification (scan the code). Ticket #raw(d.ticket_id).
]
