(module
  (type (;0;) (func))
  (memory (;0;) 1)
  (func (;0;) (type 0)
    i32.const 0
    if
      block
        block
          i32.const 1
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
)
