(module
  (type (;0;) (func))
  (func (;0;) (type 0)
    block ;; label = @1
      loop ;; label = @2
        ;; << i32.const 1234
        ;; << drop
        br 0 (;@2;)
      end
    end
  )
)
