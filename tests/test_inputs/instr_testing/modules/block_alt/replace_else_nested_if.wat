(module
  (type (;0;) (func))
  (func (;0;) (type 0)
    i32.const 0
    if
      i32.const 0
      i32.const 1
      block
        block
          if
            i32.const 0
            drop
            ;; << i32.const 12
            ;; << drop
          else ;; rm
            i32.const 1 ;; rm
            drop ;; rm
          end
        end
      end
    end
  )
  (memory (;0;) 1)
)
