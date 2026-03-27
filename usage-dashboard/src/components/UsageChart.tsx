'use client';

import { 
  LineChart, 
  Line, 
  XAxis, 
  YAxis, 
  CartesianGrid, 
  Tooltip, 
  Legend, 
  ResponsiveContainer,
  Area,
  AreaChart,
  ComposedChart,
  Bar
} from 'recharts';
import { UsageData } from '@/types';

interface UsageChartProps {
  data: UsageData[];
}

export default function UsageChart({ data }: UsageChartProps) {
  // Format data for chart
  const chartData = data.map(item => ({
    time: new Date(item.timestamp).toLocaleTimeString('en-US', { 
      hour: '2-digit', 
      minute: '2-digit' 
    }),
    kWh: Number((item.kWh * 1000).toFixed(2)), // Convert to Wh for better visualization
    XLM: Number(item.XLM.toFixed(4)),
    rate: item.rate,
    isPeakHour: item.isPeakHour
  }));

  // Custom tooltip
  const CustomTooltip = ({ active, payload, label }: any) => {
    if (active && payload && payload.length) {
      const data = payload[0].payload;
      return (
        <div className="bg-white p-3 border border-gray-200 rounded-lg shadow-lg">
          <p className="text-sm font-medium text-gray-900 mb-2">{`Time: ${label}`}</p>
          <p className="text-sm text-gray-600">
            <span className="font-medium">Usage:</span> {data.kWh} Wh
          </p>
          <p className="text-sm text-gray-600">
            <span className="font-medium">Cost:</span> {data.XLM} XLM
          </p>
          <p className="text-sm text-gray-600">
            <span className="font-medium">Rate:</span> {data.rate} XLM/kWh
          </p>
          <p className={`text-sm font-medium ${
            data.isPeakHour ? 'text-red-600' : 'text-green-600'
          }`}>
            {data.isPeakHour ? 'Peak Hour' : 'Off-Peak'}
          </p>
        </div>
      );
    }
    return null;
  };

  return (
    <div className="chart-container">
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-xl font-bold text-gray-900">Usage vs Cost Overview</h2>
        <div className="flex items-center space-x-4">
          <div className="flex items-center space-x-2">
            <div className="w-3 h-3 bg-red-500 rounded-full"></div>
            <span className="text-sm text-gray-600">Peak Hours</span>
          </div>
          <div className="flex items-center space-x-2">
            <div className="w-3 h-3 bg-green-500 rounded-full"></div>
            <span className="text-sm text-gray-600">Off-Peak</span>
          </div>
        </div>
      </div>
      
      <ResponsiveContainer width="100%" height={400}>
        <ComposedChart data={chartData}>
          <CartesianGrid strokeDasharray="3 3" stroke="#f0f0f0" />
          <XAxis 
            dataKey="time" 
            tick={{ fontSize: 12 }}
            interval="preserveStartEnd"
          />
          <YAxis 
            yAxisId="left"
            tick={{ fontSize: 12 }}
            label={{ value: 'Power (Wh)', angle: -90, position: 'insideLeft' }}
          />
          <YAxis 
            yAxisId="right" 
            orientation="right"
            tick={{ fontSize: 12 }}
            label={{ value: 'Cost (XLM)', angle: 90, position: 'insideRight' }}
          />
          <Tooltip content={<CustomTooltip />} />
          <Legend />
          
          {/* Peak hour background */}
          {chartData.map((entry, index) => {
            if (entry.isPeakHour && index > 0 && chartData[index - 1].isPeakHour) {
              return null;
            }
            if (entry.isPeakHour) {
              let endIndex = index;
              while (endIndex < chartData.length && chartData[endIndex].isPeakHour) {
                endIndex++;
              }
              return (
                <rect
                  key={`peak-${index}`}
                  x={index * (100 / chartData.length) + '%'}
                  y="0"
                  width={(endIndex - index) * (100 / chartData.length) + '%'}
                  height="100%"
                  fill="#fef2f2"
                  stroke="none"
                />
              );
            }
            return null;
          })}
          
          <Area
            yAxisId="left"
            type="monotone"
            dataKey="kWh"
            stroke="#3b82f6"
            fill="#93c5fd"
            fillOpacity={0.3}
            strokeWidth={2}
            name="Power Usage (Wh)"
          />
          
          <Line
            yAxisId="right"
            type="monotone"
            dataKey="XLM"
            stroke="#f59e0b"
            strokeWidth={3}
            dot={{ fill: '#f59e0b', r: 4 }}
            name="Cost (XLM)"
          />
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}
