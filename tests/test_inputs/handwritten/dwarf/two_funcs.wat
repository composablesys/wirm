(module
  (func $foo (param $x i32) (result i32)
    local.get $x
    i32.const 1
    i32.add
  )
  (func $bar (param $y i32) (result i32)
    local.get $y
    i32.const 2
    i32.mul
  )
  (export "foo" (func $foo))
  (export "bar" (func $bar))
)
