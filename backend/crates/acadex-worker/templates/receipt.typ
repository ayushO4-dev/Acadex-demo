// Fee receipt. Data arrives as JSON in sys.inputs.receipt, already formatted
// by the worker (amounts, dates, words). Strings inserted with #r.field are
// set as text, never parsed as markup, so names can't inject formatting.
#let r = json(bytes(sys.inputs.receipt))

#set document(title: "Fee receipt " + r.receipt_no, author: r.institution)
#set page(paper: "a4", margin: (x: 2.2cm, y: 2cm))
#set text(font: "Libertinus Serif", size: 10.5pt)

#align(center)[
  #text(size: 17pt, weight: "bold", r.institution)
  #v(-0.4em)
  #text(size: 11pt, tracking: 2pt, fill: luma(80))[FEE RECEIPT]
]
#line(length: 100%, stroke: 0.6pt)

#grid(
  columns: (1fr, 1fr),
  row-gutter: 0.75em,
  [*Receipt no.* #h(0.4em) #r.receipt_no], align(right)[*Date* #h(0.4em) #r.issued_at],
  [*Student* #h(0.4em) #r.student_name], align(right)[*Roll no.* #h(0.4em) #r.roll_no],
  grid.cell(colspan: 2)[*Program* #h(0.4em) #r.program],
)

#v(1em)
#table(
  columns: (auto, 1fr, auto),
  stroke: (x, y) => if y == 0 { (bottom: 0.6pt) } else { (bottom: 0.3pt + luma(200)) },
  inset: (x: 6pt, y: 7pt),
  align: (center, left, right),
  table.header([*No.*], [*Particulars*], [*Amount*]),
  ..r.items.enumerate().map(((i, item)) => (str(i + 1), item.particulars, item.amount)).flatten(),
  table.cell(colspan: 2, align: right, stroke: none)[*Total paid*],
  table.cell(stroke: none)[*#r.total*],
)

#v(0.5em)
*Amount in words:* #r.total_words

#v(0.5em)
Paid online by #r.method. Gateway reference: #raw(r.reference).

#v(1fr)
#line(length: 100%, stroke: 0.3pt + luma(180))
#text(size: 8.5pt, fill: luma(90))[
  This is a computer-generated receipt and needs no signature. Receipt numbers
  run without gaps through each financial year (#r.financial_year).
]
