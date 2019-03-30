/************************************************************************
 FAUST Architecture File
 Copyright (C) 2019 GRAME, Centre National de Creation Musicale
 ---------------------------------------------------------------------
 This Architecture section is free software; you can redistribute it
 and/or modify it under the terms of the GNU General Public License
 as published by the Free Software Foundation; either version 3 of
 the License, or (at your option) any later version.
 
 This program is distributed in the hope that it will be useful,
 but WITHOUT ANY WARRANTY; without even the implied warranty of
 MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 GNU General Public License for more details.
 
 You should have received a copy of the GNU General Public License
 along with this program; If not, see <http://www.gnu.org/licenses/>.
 
 EXCEPTION : As a special exception, you may create a larger work
 that contains this FAUST architecture section and distribute
 that work under terms of your choice, so long as this FAUST
 architecture section is not modified.
 ************************************************************************/

#ifndef __wasmer_dsp__
#define __wasmer_dsp__

#include <assert.h>
#include <stdint.h>
#include <errno.h>
#include <string.h>
#include <string>
#include <iostream>
#include <cmath>
#include <vector>
#include <unistd.h>

#include "../wasmer.hh"

#include "faust/dsp/dsp.h"
#include "faust/gui/JSONUIDecoder.h"

using namespace std;

class wasmer_dsp : public dsp {
    
    private:
    
        static void print_wasmer_error()
        {
            int error_len = wasmer_last_error_length();
            char* error_str = (char*)malloc(error_len);
            wasmer_last_error_message(error_str, error_len);
            printf("Error str: `%s`\n", error_str);
            free(error_str);
        }
    
        // Int: 1 function
        static int _abs(wasmer_instance_context_t* ctx, int val) { return std::abs(val); }
        
        // Float: 14 functions
        static float _acosf(wasmer_instance_context_t* ctx, float val) { return std::acos(val); }
        static float _asinf(wasmer_instance_context_t* ctx, float val) { return std::asin(val); }
        static float _atanf(wasmer_instance_context_t* ctx, float val) { return std::atan(val); }
        static float _atan2f(wasmer_instance_context_t* ctx, float v1, float v2) { return std::atan2(v1, v2); }
        static float _cosf(wasmer_instance_context_t* ctx, float val) { return std::cos(val); }
        static float _expf(wasmer_instance_context_t* ctx, float val) { return std::exp(val); }
        static float _fmodf(wasmer_instance_context_t* ctx, float v1, float v2) { return std::fmod(v1, v2); }
        static float _logf(wasmer_instance_context_t* ctx, float val) { return std::log(val); }
        static float _log10f(wasmer_instance_context_t* ctx, float val) { return std::log10(val); }
        static float _powf(wasmer_instance_context_t* ctx, float v1, float v2) { return std::pow(v1, v2); }
        static float _remainderf(wasmer_instance_context_t* ctx, float v1, float v2) { return std::remainder(v1, v2); }
        static float _roundf(wasmer_instance_context_t* ctx, float val) { return std::round(val); }
        static float _sinf(wasmer_instance_context_t* ctx, float val) { return std::sin(val); }
        static float _tanf(wasmer_instance_context_t* ctx, float val) { return std::tan(val); }
    
        // Double: 14 functions
        static double _acos(wasmer_instance_context_t* ctx, double val) { return std::acos(val); }
        static double _asin(wasmer_instance_context_t* ctx, double val) { return std::asin(val); }
        static double _atan(wasmer_instance_context_t* ctx, double val) { return std::atan(val); }
        static double _atan2(wasmer_instance_context_t* ctx, double v1, double v2) { return std::atan2(v1, v2); }
        static double _cos(wasmer_instance_context_t* ctx, double val) { return std::cos(val); }
        static double _exp(wasmer_instance_context_t* ctx, double val) { return std::exp(val); }
        static double _fmod(wasmer_instance_context_t* ctx, double v1, double v2) { return std::fmod(v1, v2); }
        static double _log(wasmer_instance_context_t* ctx, double val) { return std::log(val); }
        static double _log10(wasmer_instance_context_t* ctx, double val) { return std::log10(val); }
        static double _pow(wasmer_instance_context_t* ctx, double v1, double v2) { return std::pow(v1, v2); }
        static double _remainder(wasmer_instance_context_t* ctx, double v1, double v2) { return std::remainder(v1, v2); }
        static double _round(wasmer_instance_context_t* ctx, double val) { return std::round(val); }
        static double _sin(wasmer_instance_context_t* ctx, double val) { return std::sin(val); }
        static double _tan(wasmer_instance_context_t* ctx, double val) { return std::tan(val); }
    
        // Float
        typedef float (*math_unary_float_fun)(wasmer_instance_context_t* ctx, float val);
        typedef float (*math_binary_float_fun)(wasmer_instance_context_t* ctx, float val1, float val2);
        
        // double
        typedef double (*math_unary_double_fun)(wasmer_instance_context_t* ctx, double val);
        typedef double (*math_binary_double_fun)(wasmer_instance_context_t* ctx, double val1, double val2);
        
        typedef int (*math_unary_int_fun)(wasmer_instance_context_t* ctx, int val);

        wasmer_module_t* fModule;
        wasmer_instance_t* fInstance;
        
        int fWasmInputs;        // Index in wasm memory
        int fWasmOutputs;       // Index in wasm memory
        
        FAUSTFLOAT** fInputs;   // Wasm memory mapped to pointers
        FAUSTFLOAT** fOutputs;  // Wasm memory mapped to pointers
        
        char* fMemory;
        JSONUITemplatedDecoder* fDecoder;
        
        vector<wasmer_import_func_t*> fFunctionList;
        
        wasmer_byte_array fModuleNameBytes;
    
        template <class T, wasmer_value_tag REAL>
        wasmer_import_t createRealUnary(const char* import_name, T fun)
        {
            wasmer_value_tag params_sig[] = { REAL };
            wasmer_value_tag returns_sig[] = { REAL };
            
            wasmer_byte_array import_name_bytes;
            import_name_bytes.bytes = (const uint8_t*)import_name;
            import_name_bytes.bytes_len = strlen(import_name);
            
            wasmer_import_func_t* func = wasmer_import_func_new((void (*)(void *))fun, params_sig, 1, returns_sig, 1);
            fFunctionList.push_back(func);
            
            wasmer_import_t func_import;
            func_import.module_name = fModuleNameBytes;
            func_import.import_name = import_name_bytes;
            func_import.tag = wasmer_import_export_kind::WASM_FUNCTION;
            func_import.value.func = func;
            
            return func_import;
        }
    
        template <class T, wasmer_value_tag REAL>
        wasmer_import_t createRealBinary(const char* import_name, T fun)
        {
            wasmer_value_tag params_sig[] = { REAL, REAL };
            wasmer_value_tag returns_sig[] = { REAL };
            
            wasmer_byte_array import_name_bytes;
            import_name_bytes.bytes = (const uint8_t*)import_name;
            import_name_bytes.bytes_len = strlen(import_name);
            
            wasmer_import_func_t* func = wasmer_import_func_new((void (*)(void *))fun, params_sig, 2, returns_sig, 1);
            fFunctionList.push_back(func);
            
            wasmer_import_t func_import;
            func_import.module_name = fModuleNameBytes;
            func_import.import_name = import_name_bytes;
            func_import.tag = wasmer_import_export_kind::WASM_FUNCTION;
            func_import.value.func = func;
            
            return func_import;
        }
    
        wasmer_import_t createFloatUnary(const char* import_name, math_unary_float_fun fun)
        {
            return createRealUnary<math_unary_float_fun, wasmer_value_tag::WASM_F32>(import_name, fun);
        }
        wasmer_import_t createDoubleUnary(const char* import_name, math_unary_double_fun fun)
        {
            return createRealUnary<math_unary_double_fun, wasmer_value_tag::WASM_F64>(import_name, fun);
        }
    
        wasmer_import_t createFloatBinary(const char* import_name, math_binary_float_fun fun)
        {
            return createRealBinary<math_binary_float_fun, wasmer_value_tag::WASM_F32>(import_name, fun);
        }
        wasmer_import_t createDoubleBinary(const char* import_name, math_binary_double_fun fun)
        {
            return createRealBinary<math_binary_double_fun, wasmer_value_tag::WASM_F64>(import_name, fun);
        }
    
        wasmer_import_t createIntUnary(const char* import_name, math_unary_int_fun fun)
        {
            wasmer_value_tag params_sig[] = { wasmer_value_tag::WASM_I32 };
            wasmer_value_tag returns_sig[] = { wasmer_value_tag::WASM_I32 };
            
            wasmer_byte_array import_name_bytes;
            import_name_bytes.bytes = (const uint8_t*)import_name;
            import_name_bytes.bytes_len = strlen(import_name);
            
            wasmer_import_func_t* func = wasmer_import_func_new((void (*)(void *))fun, params_sig, 1, returns_sig, 1);
            fFunctionList.push_back(func);
            
            wasmer_import_t func_import;
            func_import.module_name = fModuleNameBytes;
            func_import.import_name = import_name_bytes;
            func_import.tag = wasmer_import_export_kind::WASM_FUNCTION;
            func_import.value.func = func;
            
            return func_import;
        }
        
    public:

        wasmer_dsp(const string& filename)
        {
            std::ifstream is(filename, std::ifstream::binary);
            is.seekg(0, is.end);
            int len = is.tellg();
            is.seekg(0, is.beg);
            char* bytes = new char[len];
            is.read(bytes, len);
            
            fModule = nullptr;
            wasmer_result_t compile_result = wasmer_compile(&fModule, (uint8_t*)bytes, len);
            assert(compile_result == wasmer_result_t::WASMER_OK);
            
            // Env
            const char* module_name = "env";
            fModuleNameBytes.bytes = (const uint8_t*)module_name;
            fModuleNameBytes.bytes_len = strlen(module_name);
            
            wasmer_import_t imports[] = {
                
                // Int
                createIntUnary("_abs", _abs),
                
                // Float
                createFloatUnary("_acosf", _acosf),
                createFloatUnary("_asinf", _asinf),
                createFloatUnary("_atanf", _atanf),
                createFloatBinary("_atan2f", _atan2f),
                createFloatUnary("_cosf", _cosf),
                createFloatUnary("_expf", _expf),
                createFloatBinary("_fmodf", _fmodf),
                createFloatUnary("_logf", _logf),
                createFloatUnary("_log10f", _log10f),
                createFloatBinary("_powf", _powf),
                createFloatBinary("_remainderf", _remainderf),
                createFloatUnary("_roundf", _roundf),
                createFloatUnary("_sinf", _sinf),
                createFloatUnary("_tanf", _tanf),
                
                // Double
                createDoubleUnary("_acos", _acos),
                createDoubleUnary("_asin", _asin),
                createDoubleUnary("_atan", _atan),
                createDoubleBinary("_atan2", _atan2),
                createDoubleUnary("_cos", _cos),
                createDoubleUnary("_exp", _exp),
                createDoubleBinary("_fmod", _fmod),
                createDoubleUnary("_log", _log),
                createDoubleUnary("_log10", _log10),
                createDoubleBinary("_pow", _pow),
                createDoubleBinary("_remainder", _remainder),
                createDoubleUnary("_round", _round),
                createDoubleUnary("_sin", _sin),
                createDoubleUnary("_tan", _tan),
            };
            
            fInstance = nullptr;
            wasmer_result_t instantiate_result = wasmer_module_instantiate(fModule, &fInstance, imports, 1 + 14 * 2);
            if (instantiate_result != wasmer_result_t::WASMER_OK) print_wasmer_error();
            
            assert(instantiate_result == wasmer_result_t::WASMER_OK);
            
            // Get all exports.
            wasmer_exports_t* exports = nullptr;
            wasmer_instance_exports(fInstance, &exports);
            
            // Memory is as index 7
            wasmer_export_t* export1 = wasmer_exports_get(exports, 7);
            wasmer_import_export_kind kind = wasmer_export_kind(export1);
            assert(kind == wasmer_import_export_kind::WASM_MEMORY);
            
            // Cast the export into a memory.
            wasmer_memory_t* memory;
            wasmer_result_t export_to_memory_result = wasmer_export_to_memory(export1, &memory);
            assert(export_to_memory_result == wasmer_result_t::WASMER_OK);
            
            // JSON is located at offset 0 in the memory segment
            fMemory = (char*)wasmer_memory_data(memory);
            string json = string(fMemory);
            
            fDecoder = createJSONUIDecoder(json);
            
            std::cout << "Libfaust version: " << fDecoder->getLibVersion() << std::endl;
            std::cout << "Compilation options: " << fDecoder->getCompileOptions() << std::endl;
            
            int ptr_size = sizeof(FAUSTFLOAT*);
            int sample_size = sizeof(FAUSTFLOAT);
            int buffer_size = 4096; // Max
            
            fInputs = new FAUSTFLOAT*[fDecoder->getNumInputs()];
            fOutputs = new FAUSTFLOAT*[fDecoder->getNumOutputs()];
            
            // DSP is placed first with index 0. Audio buffer start at the end of DSP.
            int audio_heap_ptr = fDecoder->getDSPSize();
            
            // Setup pointers offset
            int audio_heap_ptr_inputs = audio_heap_ptr;
            int audio_heap_ptr_outputs = audio_heap_ptr_inputs + (fDecoder->getNumInputs() * ptr_size);
            
            // Setup buffer offset
            int audio_heap_inputs = audio_heap_ptr_outputs + (fDecoder->getNumOutputs() * ptr_size);
            int audio_heap_outputs = audio_heap_inputs + (fDecoder->getNumInputs() * buffer_size * sample_size);
            
            if (fDecoder->getNumInputs() > 0) {
                
                fWasmInputs = audio_heap_ptr_inputs;
                int* HEAP32 = reinterpret_cast<int*>(fMemory + audio_heap_ptr_inputs);
                FAUSTFLOAT* HEAPF32 = reinterpret_cast<FAUSTFLOAT*>(fMemory + audio_heap_inputs);
                
                for (int i = 0; i < fDecoder->getNumInputs(); i++) {
                    // Setup input buffer indexes for wasm side
                    HEAP32[i] = audio_heap_inputs + (buffer_size * sample_size * i);
                    // Setup input buffer pointers for runtime side
                    fInputs[i] = HEAPF32 + (buffer_size * i);
                }
            }
            
            if (fDecoder->getNumOutputs() > 0) {
                
                fWasmOutputs = audio_heap_ptr_outputs;
                int* HEAP32 = reinterpret_cast<int*>(fMemory + audio_heap_ptr_outputs);
                FAUSTFLOAT* HEAPF32 = reinterpret_cast<FAUSTFLOAT*>(fMemory + audio_heap_outputs);
                
                for (int i = 0; i < fDecoder->getNumOutputs(); i++) {
                    // Setup output buffer indexes for wasm side
                    HEAP32[i] = audio_heap_outputs + (buffer_size * sample_size * i);
                    // Setup output buffer pointers for runtime side
                    fOutputs[i] =  HEAPF32 + (buffer_size * i);
                }
            }
            
            delete[] bytes;
            wasmer_exports_destroy(exports);
        }
        
        virtual ~wasmer_dsp()
        {
            wasmer_instance_destroy(fInstance);
            wasmer_module_destroy(fModule);
            for (auto& it : fFunctionList) {
                wasmer_import_func_destroy(it);
            }
            delete fDecoder;
        }
        
        virtual int getNumInputs()
        {
            wasmer_value_t param_dsp;
            param_dsp.tag = wasmer_value_tag::WASM_I32;
            param_dsp.value.I32 = 0;
            
            wasmer_value_t params[] = { param_dsp };
            
            wasmer_value_t result_one;
            wasmer_value_t results[] = { result_one };
            wasmer_result_t call_result = wasmer_instance_call(fInstance, "getNumInputs", params, 1, results, 1);
            
            return results[0].value.I32;
        }
        
        virtual int getNumOutputs()
        {
            wasmer_value_t param_dsp;
            param_dsp.tag = wasmer_value_tag::WASM_I32;
            param_dsp.value.I32 = 0;
            
            wasmer_value_t params[] = { param_dsp };
            
            wasmer_value_t result_one;
            wasmer_value_t results[] = { result_one };
            wasmer_result_t call_result = wasmer_instance_call(fInstance, "getNumOutputs", params, 1, results, 1);
            
            return results[0].value.I32;
        }
        
        virtual void buildUserInterface(UI* ui_interface)
        {
            fDecoder->buildUserInterface(ui_interface, fMemory);
        }
        
        virtual int getSampleRate()
        {
            wasmer_value_t param_dsp;
            param_dsp.tag = wasmer_value_tag::WASM_I32;
            param_dsp.value.I32 = 0;
            
            wasmer_value_t params[] = { param_dsp };
            
            wasmer_value_t result_one;
            wasmer_value_t results[] = { result_one };
            wasmer_result_t call_result = wasmer_instance_call(fInstance, "getSampleRate", params, 1, results, 1);
            
            return results[0].value.I32;
        }
        
        virtual void init(int sample_rate)
        {
            wasmer_value_t param_dsp;
            param_dsp.tag = wasmer_value_tag::WASM_I32;
            param_dsp.value.I32 = 0;
            
            wasmer_value_t param_sr;
            param_sr.tag = wasmer_value_tag::WASM_I32;
            param_sr.value.I32 = sample_rate;
            
            wasmer_value_t params[] = { param_dsp, param_sr };
            
            wasmer_value_t results[] = {};
            wasmer_result_t call_result = wasmer_instance_call(fInstance, "init", params, 2, results, 0);
        }
        
        virtual void instanceInit(int sample_rate)
        {
            wasmer_value_t param_dsp;
            param_dsp.tag = wasmer_value_tag::WASM_I32;
            param_dsp.value.I32 = 0;
            
            wasmer_value_t param_sr;
            param_sr.tag = wasmer_value_tag::WASM_I32;
            param_sr.value.I32 = sample_rate;
            
            wasmer_value_t params[] = { param_dsp, param_sr };
            
            wasmer_value_t results[] = {};
            wasmer_result_t call_result = wasmer_instance_call(fInstance, "instanceInit", params, 2, results, 0);
        }
        
        virtual void instanceConstants(int sample_rate)
        {
            wasmer_value_t param_dsp;
            param_dsp.tag = wasmer_value_tag::WASM_I32;
            param_dsp.value.I32 = 0;
            
            wasmer_value_t param_sr;
            param_sr.tag = wasmer_value_tag::WASM_I32;
            param_sr.value.I32 = sample_rate;
            
            wasmer_value_t params[] = { param_dsp, param_sr };
            
            wasmer_value_t results[] = {};
            wasmer_result_t call_result = wasmer_instance_call(fInstance, "instanceConstants", params, 2, results, 0);
        }
        
        virtual void instanceResetUserInterface()
        {
            wasmer_value_t param_dsp;
            param_dsp.tag = wasmer_value_tag::WASM_I32;
            param_dsp.value.I32 = 0;
            
            wasmer_value_t params[] = { param_dsp };
            
            wasmer_value_t results[] = {};
            wasmer_result_t call_result = wasmer_instance_call(fInstance, "instanceResetUserInterface", params, 1, results, 0);
        }
        
        virtual void instanceClear()
        {
            wasmer_value_t param_dsp;
            param_dsp.tag = wasmer_value_tag::WASM_I32;
            param_dsp.value.I32 = 0;
            
            wasmer_value_t params[] = { param_dsp };
            
            wasmer_value_t results[] = {};
            wasmer_result_t call_result = wasmer_instance_call(fInstance, "instanceClear", params, 1, results, 0);
        }
       
        virtual dsp* clone()
        {
            assert(false);
            return nullptr;
        }
       
        virtual void metadata(Meta* m)
        {
            fDecoder->metadata(m);
        }
        
        virtual void compute(int count, FAUSTFLOAT** inputs, FAUSTFLOAT** outputs)
        {
            wasmer_value_t param_dsp;
            param_dsp.tag = wasmer_value_tag::WASM_I32;
            param_dsp.value.I32 = 0;
            
            wasmer_value_t param_count;
            param_count.tag = wasmer_value_tag::WASM_I32;
            param_count.value.I32 = count;
            
            wasmer_value_t param_inputs;
            param_inputs.tag = wasmer_value_tag::WASM_I32;
            param_inputs.value.I32 = fWasmInputs;

            wasmer_value_t param_outputs;
            param_outputs.tag = wasmer_value_tag::WASM_I32;
            param_outputs.value.I32 = fWasmOutputs;
      
            wasmer_value_t params[] = { param_dsp, param_count, param_inputs, param_outputs };
            wasmer_value_t results[] = {};
            
            // Copy audio inputs
            for (int i = 0; i < fDecoder->getNumInputs(); i++) {
                memcpy(fInputs[i], inputs[i], sizeof(FAUSTFLOAT) * count);
            }
            
            // Call wasm code
            wasmer_result_t call_result = wasmer_instance_call(fInstance, "compute", params, 4, results, 0);
            
            // Copy audio outputs
            for (int i = 0; i < fDecoder->getNumOutputs(); i++) {
                memcpy(outputs[i], fOutputs[i], sizeof(FAUSTFLOAT) * count);
            }
        }
        
        virtual void compute(double /*date_usec*/, int count, FAUSTFLOAT** inputs, FAUSTFLOAT** outputs)
        {
            compute(count, inputs, outputs);
        }

};

#endif
