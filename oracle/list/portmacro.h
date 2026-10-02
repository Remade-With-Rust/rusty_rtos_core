/* A port with nothing in it: list.c needs only the types. */
#ifndef PORTMACRO_H
#define PORTMACRO_H
#include <stdint.h>
#define portCHAR          char
#define portFLOAT         float
#define portDOUBLE        double
#define portLONG          long
#define portSHORT         short
#define portSTACK_TYPE    uint32_t
#define portBASE_TYPE     long
typedef portSTACK_TYPE StackType_t;
typedef long BaseType_t;
typedef unsigned long UBaseType_t;
typedef uint32_t TickType_t;
#define portMAX_DELAY              ( TickType_t ) 0xffffffffUL
#define portTICK_TYPE_IS_ATOMIC    1
#define portSTACK_GROWTH          ( -1 )
#define portTICK_PERIOD_MS        ( ( TickType_t ) 1000 / configTICK_RATE_HZ )
#define portBYTE_ALIGNMENT        8
#define portYIELD()
#define portENTER_CRITICAL()
#define portEXIT_CRITICAL()
#define portDISABLE_INTERRUPTS()
#define portENABLE_INTERRUPTS()
#define portSET_INTERRUPT_MASK_FROM_ISR()         0
#define portCLEAR_INTERRUPT_MASK_FROM_ISR( x )    ( ( void ) ( x ) )
#define portTASK_FUNCTION_PROTO( vFunction, pvParameters )    void vFunction( void * pvParameters )
#define portTASK_FUNCTION( vFunction, pvParameters )          void vFunction( void * pvParameters )
#define portNOP()
#define portMEMORY_BARRIER()
#endif
