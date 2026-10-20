import {test} from 'node:test';
import assert from 'node:assert/strict';
import {CpuAdmission} from '../src/controllers/cpuadmission';
import {CapacityUnavailable} from '../src/controllers/capacity';
test('CPU admission blocks sustained load and only resumes below the low watermark',()=>{
 const cpu=new CpuAdmission(80,65);cpu.sample(0,0);cpu.sample(5,100);
 assert.throws(()=>cpu.assertAvailable(),CapacityUnavailable);
 cpu.sample(30,200);assert.throws(()=>cpu.assertAvailable(),CapacityUnavailable);
 cpu.sample(90,300);cpu.assertAvailable();
 assert.equal(cpu.status().blocked,false);
});
test('CPU counters resetting do not manufacture overload',()=>{
 const cpu=new CpuAdmission();cpu.sample(100,200);cpu.sample(0,0);cpu.sample(80,100);cpu.assertAvailable();
 assert.throws(()=>new CpuAdmission(65,80),/Invalid/);
});
