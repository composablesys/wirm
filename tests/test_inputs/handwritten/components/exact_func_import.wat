(component
  (core module $a
    (type $f (func))
    (func (export "f") (type $f)))
  (core module $r
    (type $f (func))
    (import "a" "f" (func (exact (type $f))))
    (export "f" (func 0)))
  (core instance $a (instantiate $a))
  (core instance $r (instantiate $r (with "a" (instance $a)))))
