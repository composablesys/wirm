(component
  (core rec
    (type $a (sub (descriptor $d) (struct (field (ref null $o)))))
    (type $d (sub (describes $a) (struct (field (ref null $o)))))
    (type $o (sub (struct)))
  )
)
